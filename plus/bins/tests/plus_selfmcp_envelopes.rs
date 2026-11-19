//! Contract per self-MCP tool and resource: one golden result per case under
//! `plus/fixtures/selfmcp-envelopes/`, produced by the real `toolport-selfmcp` stdio server in
//! the synthetic world of `common/world.rs`. The GUI and agents parse these results, so a change
//! in their shape must be a reviewed change: `CTL_ENVELOPE_BLESS=1 cargo test -p
//! toolport-plus-bins --test plus_selfmcp_envelopes` rewrites the goldens (rejected when `CI` is
//! set).
//!
//! Adding a tool: `case("<tool>", &[read(...), refused(...), write(...)])`. A call that only reads
//! must leave the world as it was, so must a refusal and a failure. A resource needs no case:
//! every entry of `RESOURCES` is read in the same world.

use std::collections::BTreeSet;

use serde_json::{json, Value};
use toolport_plus::selfmcp::{RESOURCES, TOOLS};

mod common;
use common::attention_world::{attention_home, attention_updates};
use common::catalog_world;
use common::client::Client;
use common::compression;
use common::library_world::{self, library_home};
use common::server_repo_world::{server_repo, server_repo_diverged, server_repo_with_peers};
use common::taps_world;
use common::tasks_world::{scheduler_home, tasks_home};
use common::world::{World, FAKE_SECRET, SECRET_KEY};
use common::{golden, normalize};

#[derive(Clone, Copy, PartialEq)]
enum Expect {
    Ok,
    Error(&'static str),
}

#[derive(Clone, Copy)]
struct Call {
    label: &'static str,
    tool: &'static str,
    args: &'static str,
    expect: Expect,
    writes: bool,
    golden: bool,
    hook: Option<fn(&World)>,
}

/// A call that succeeds and leaves the world as it was: a read or a dry run.
const fn read(label: &'static str, tool: &'static str, args: &'static str) -> Call {
    Call {
        label,
        tool,
        args,
        expect: Expect::Ok,
        writes: false,
        golden: true,
        hook: None,
    }
}

/// Reshapes the world for the calls after it; makes no call and records no golden.
const fn hook(label: &'static str, prepare: fn(&World)) -> Call {
    Call {
        golden: false,
        hook: Some(prepare),
        ..read(label, "", "{}")
    }
}

/// A call that fails with this error kind and changes nothing.
const fn fails(
    label: &'static str,
    kind: &'static str,
    tool: &'static str,
    args: &'static str,
) -> Call {
    Call {
        expect: Expect::Error(kind),
        ..read(label, tool, args)
    }
}

/// A call that changes the world and succeeds.
const fn write(label: &'static str, tool: &'static str, args: &'static str) -> Call {
    Call {
        writes: true,
        ..read(label, tool, args)
    }
}

/// A call that must be refused for want of `confirm`, and changes nothing.
const fn refused(label: &'static str, tool: &'static str, args: &'static str) -> Call {
    fails(label, "refused", tool, args)
}

/// A call that prepares the world for the ones after it: no golden.
const SETUP_EXPORT_APPLY: &str =
    r#"{"out":"{base}/out/a.zip","roots":["{home}/code"],"dry_run":false,"confirm":true}"#;

const fn setup(tool: &'static str, args: &'static str) -> Call {
    Call {
        golden: false,
        ..write(tool, tool, args)
    }
}

const fn setup_as(label: &'static str, tool: &'static str, args: &'static str) -> Call {
    Call {
        label,
        ..setup(tool, args)
    }
}

const ENABLE_PROXY_PORT: &str =
    r#"{"provider":"headroom","port":29214,"dry_run":false,"confirm":true}"#;
const ENABLE_OFF_PORT: &str =
    r#"{"provider":"headroom","port":29213,"dry_run":false,"confirm":true}"#;
const TEARDOWN: &str = r#"{"teardown":true}"#;
const TEARDOWN_UNCONFIRMED: &str = r#"{"teardown":true,"dry_run":false}"#;

const GAMMA: &str = r#"{"name":"gamma","config":{"command":"gamma-mcp","args":["--flag"]}}"#;
const GAMMA_APPLY: &str =
    r#"{"name":"gamma","config":{"command":"gamma-mcp","args":["--flag"]},"confirm":true}"#;

struct Case {
    tool: &'static str,
    prepare: Option<fn(&World)>,
    calls: &'static [Call],
}

const fn case(tool: &'static str, calls: &'static [Call]) -> Case {
    Case {
        tool,
        calls,
        prepare: None,
    }
}

const fn setup_case(tool: &'static str, calls: &'static [Call]) -> Case {
    Case {
        prepare: Some(common::setup_world::setup_home),
        ..case(tool, calls)
    }
}

/// A case whose world first gets the skills repository, CLAUDE.md files and project folder.
const fn context_case(tool: &'static str, calls: &'static [Call]) -> Case {
    Case {
        prepare: Some(World::context_files),
        ..case(tool, calls)
    }
}

/// A case that starts in a world `prepare` has reshaped.
const fn prepared(tool: &'static str, prepare: fn(&World), calls: &'static [Call]) -> Case {
    Case {
        prepare: Some(prepare),
        ..case(tool, calls)
    }
}

const SELECT_TAP: &str = r#"{"repo":"{base}/select-src","name":"picks","dry_run":false}"#;
const SELECT_CHOOSE: &str =
    r#"{"source":"picks","on":["code-review","agent:reviewer"],"dry_run":false,"confirm":true}"#;
const BUNDLE: &str = r#"{"bundle_path":"{repo}/skills-repo-bundle.zip"}"#;
const ADD_TAP: &str = r#"{"repo":"{base}/tap-src","name":"local","dry_run":false}"#;
const APPLY: &str = r#"{"dry_run":false,"confirm":true}"#;
const UNCONFIRMED: &str = r#"{"dry_run":false}"#;
const CLAUDE: &str = r#"{"client_keys":["claude-code"]}"#;
const CLAUDE_UNCONFIRMED: &str = r#"{"client_keys":["claude-code"],"dry_run":false}"#;
const CLAUDE_UNATTENDED: &str = r#"{"client_keys":["claude-code"],"unattended":true}"#;
const CLAUDE_APPLY: &str = r#"{"client_keys":["claude-code"],"dry_run":false,"confirm":true}"#;

const AIDER: &str = r#"{"name":"plain","client_keys":["aider"],"dry_run":false,"confirm":true}"#;
const AIDER_STYLE: &str = r#"{"name":"plain","client_keys":["aider"]}"#;
const AIDER_STYLE_UNCONFIRMED: &str = r#"{"name":"plain","client_keys":["aider"],"dry_run":false}"#;
const AIDER_REMOVE_PREVIEW: &str = r#"{"client_keys":["aider"]}"#;
const AIDER_REMOVE_UNCONFIRMED: &str = r#"{"client_keys":["aider"],"dry_run":false}"#;
const AIDER_REMOVE: &str = r#"{"client_keys":["aider"],"dry_run":false,"confirm":true}"#;

const CASES: &[Case] = &[
    prepared(
        "attention_list",
        attention_home,
        &[
            read("busy", "attention_list", "{}"),
            setup(
                "attention_dismiss",
                r#"{"id":"tasks:failed:nightly-report"}"#,
            ),
            read("hidden", "attention_list", r#"{"hidden":true}"#),
        ],
    ),
    prepared(
        "attention_snooze",
        attention_home,
        &[
            write(
                "hour",
                "attention_snooze",
                r#"{"id":"library:held","for":"1h"}"#,
            ),
            fails(
                "too_long",
                "invalid_arguments",
                "attention_snooze",
                r#"{"id":"library:held","for":"8d"}"#,
            ),
            fails(
                "unknown_row",
                "not_found",
                "attention_snooze",
                r#"{"id":"no-such-row"}"#,
            ),
            fails("missing_id", "invalid_arguments", "attention_snooze", "{}"),
        ],
    ),
    prepared(
        "attention_skip",
        attention_updates,
        &[
            setup("attention_check", "{}"),
            fails(
                "without_a_version",
                "invalid_arguments",
                "attention_skip",
                r#"{"id":"tasks:failed:nightly-report"}"#,
            ),
            write(
                "skip",
                "attention_skip",
                r#"{"id":"updates:tap:team-skills:3c5d9d8e>60abf5fe"}"#,
            ),
        ],
    ),
    prepared(
        "attention_dismiss",
        attention_home,
        &[
            read(
                "preview",
                "attention_dismiss",
                r#"{"id":"tasks:failed:nightly-report","dry_run":true}"#,
            ),
            write(
                "dismiss",
                "attention_dismiss",
                r#"{"id":"tasks:failed:nightly-report"}"#,
            ),
            fails(
                "needs_you",
                "invalid_arguments",
                "attention_dismiss",
                r#"{"id":"library:held"}"#,
            ),
        ],
    ),
    prepared(
        "attention_restore",
        attention_home,
        &[
            fails(
                "not_hidden",
                "not_found",
                "attention_restore",
                r#"{"id":"library:held"}"#,
            ),
            setup(
                "attention_dismiss",
                r#"{"id":"tasks:failed:nightly-report"}"#,
            ),
            write(
                "restore",
                "attention_restore",
                r#"{"id":"tasks:failed:nightly-report"}"#,
            ),
        ],
    ),
    prepared(
        "attention_undo",
        attention_home,
        &[
            fails("nothing", "not_found", "attention_undo", "{}"),
            setup(
                "attention_dismiss",
                r#"{"id":"tasks:failed:nightly-report"}"#,
            ),
            write("undo", "attention_undo", "{}"),
        ],
    ),
    prepared(
        "attention_check",
        attention_home,
        &[write("no_sources", "attention_check", "{}")],
    ),
    prepared(
        "attention_settings",
        attention_home,
        &[read("defaults", "attention_settings", "{}")],
    ),
    prepared(
        "attention_set",
        attention_home,
        &[
            write(
                "check_off",
                "attention_set",
                r#"{"key":"check.updates","on":false}"#,
            ),
            write(
                "notify_on",
                "attention_set",
                r#"{"key":"notify","on":true}"#,
            ),
            fails(
                "unknown",
                "invalid_arguments",
                "attention_set",
                r#"{"key":"check.nothing","on":true}"#,
            ),
            fails(
                "missing_on",
                "invalid_arguments",
                "attention_set",
                r#"{"key":"notify"}"#,
            ),
        ],
    ),
    prepared(
        "tasks_list",
        tasks_home,
        &[
            read("enabled", "tasks_list", "{}"),
            read("all", "tasks_list", r#"{"all":true}"#),
        ],
    ),
    prepared(
        "tasks_get",
        tasks_home,
        &[
            read("waiting", "tasks_get", r#"{"id":"portal-token"}"#),
            fails("unknown", "not_found", "tasks_get", r#"{"id":"nothing"}"#),
            fails("no_id", "invalid_arguments", "tasks_get", "{}"),
        ],
    ),
    prepared(
        "tasks_history",
        tasks_home,
        &[
            read("all", "tasks_history", "{}"),
            read("one_run", "tasks_history", r#"{"run":"run-fixture-ok"}"#),
            fails(
                "unknown_run",
                "not_found",
                "tasks_history",
                r#"{"run":"run-nothing"}"#,
            ),
        ],
    ),
    prepared(
        "tasks_missed",
        scheduler_home,
        &[
            read("all", "tasks_missed", "{}"),
            read("one", "tasks_missed", r#"{"id":"portal-token"}"#),
        ],
    ),
    prepared(
        "tasks_set_enabled",
        tasks_home,
        &[
            read(
                "preview",
                "tasks_set_enabled",
                r#"{"id":"draft-cleanup","enabled":true,"dry_run":true}"#,
            ),
            Call {
                writes: true,
                ..fails(
                    "no_broker",
                    "approval_unavailable",
                    "tasks_set_enabled",
                    r#"{"id":"draft-cleanup","enabled":true}"#,
                )
            },
            write(
                "off",
                "tasks_set_enabled",
                r#"{"id":"nightly-report","enabled":false}"#,
            ),
            fails(
                "no_enabled",
                "invalid_arguments",
                "tasks_set_enabled",
                r#"{"id":"nightly-report"}"#,
            ),
            fails(
                "unknown",
                "not_found",
                "tasks_set_enabled",
                r#"{"id":"nothing","enabled":false}"#,
            ),
        ],
    ),
    prepared(
        "tasks_run",
        tasks_home,
        &[
            read(
                "preview",
                "tasks_run",
                r#"{"id":"portal-token","dry_run":true}"#,
            ),
            Call {
                writes: true,
                ..fails(
                    "no_broker",
                    "approval_unavailable",
                    "tasks_run",
                    r#"{"id":"portal-token"}"#,
                )
            },
            fails(
                "not_allowed",
                "conflict",
                "tasks_run",
                r#"{"id":"nightly-report"}"#,
            ),
            fails(
                "unknown",
                "not_found",
                "tasks_run",
                r#"{"id":"nothing","dry_run":true}"#,
            ),
        ],
    ),
    prepared(
        "tasks_cancel",
        tasks_home,
        &[
            write("stale", "tasks_cancel", r#"{"run":"run-fixture-stale"}"#),
            fails(
                "ended",
                "conflict",
                "tasks_cancel",
                r#"{"run":"run-fixture-ok"}"#,
            ),
            fails(
                "unknown",
                "not_found",
                "tasks_cancel",
                r#"{"run":"run-nothing"}"#,
            ),
        ],
    ),
    case(
        "where_am_i",
        &[
            read("", "where_am_i", "{}"),
            fails(
                "invalid",
                "invalid_arguments",
                "where_am_i",
                r#"{"depth":2}"#,
            ),
            fails("wrong-type", "invalid_arguments", "where_am_i", r#"[1]"#),
        ],
    ),
    case("doctor", &[read("", "doctor", "{}")]),
    case(
        "skills_edit_body",
        &[
            refused(
                "refused",
                "skills_edit_body",
                r#"{"name":"demo","new_body":"Changed body\n"}"#,
            ),
            write(
                "apply",
                "skills_edit_body",
                r#"{"name":"demo","new_body":"Changed body\n","confirm":true}"#,
            ),
            fails(
                "missing",
                "not_found",
                "skills_edit_body",
                r#"{"name":"nope","new_body":"x","confirm":true}"#,
            ),
        ],
    ),
    case(
        "skills_edit_frontmatter",
        &[
            refused(
                "refused",
                "skills_edit_frontmatter",
                r#"{"name":"demo","patch":{"description":"Edited description"}}"#,
            ),
            write(
                "apply",
                "skills_edit_frontmatter",
                r#"{"name":"demo","patch":{"description":"Edited description"},"confirm":true}"#,
            ),
            fails(
                "missing",
                "not_found",
                "skills_edit_frontmatter",
                r#"{"name":"nope","patch":{"description":"x"},"confirm":true}"#,
            ),
        ],
    ),
    case(
        "skills_delete",
        &[
            refused("refused", "skills_delete", r#"{"name":"demo"}"#),
            write(
                "apply",
                "skills_delete",
                r#"{"name":"demo","confirm":true}"#,
            ),
            fails(
                "gone",
                "not_found",
                "skills_delete",
                r#"{"name":"demo","confirm":true}"#,
            ),
        ],
    ),
    case("agents_list", &[read("", "agents_list", "{}")]),
    case(
        "agents_get",
        &[
            read("found", "agents_get", r#"{"name":"helper"}"#),
            fails("missing", "not_found", "agents_get", r#"{"name":"nope"}"#),
        ],
    ),
    case("agents_lint", &[read("", "agents_lint", "{}")]),
    case(
        "agents_list_transpilers",
        &[read("", "agents_list_transpilers", "{}")],
    ),
    case(
        "agents_scaffold",
        &[
            write(
                "apply",
                "agents_scaffold",
                r#"{"name":"scout","model":"sonnet"}"#,
            ),
            fails(
                "exists",
                "conflict",
                "agents_scaffold",
                r#"{"name":"scout"}"#,
            ),
            fails(
                "escaping_name",
                "invalid_arguments",
                "agents_scaffold",
                r#"{"name":"../escape"}"#,
            ),
        ],
    ),
    case(
        "agents_sync",
        &[
            read("preview", "agents_sync", CLAUDE),
            refused("refused", "agents_sync", CLAUDE_UNCONFIRMED),
            write("apply", "agents_sync", CLAUDE_APPLY),
        ],
    ),
    case(
        "agents_diff",
        &[
            read("fresh", "agents_diff", "{}"),
            setup("agents_sync", CLAUDE_APPLY),
            read("synced", "agents_diff", "{}"),
        ],
    ),
    case("agents_audit", &[read("", "agents_audit", "{}")]),
    case(
        "agents_status",
        &[
            read("fresh", "agents_status", "{}"),
            setup("agents_sync", CLAUDE_APPLY),
            read("synced", "agents_status", "{}"),
        ],
    ),
    case(
        "agents_clean",
        &[
            setup("agents_sync", CLAUDE_APPLY),
            read("preview", "agents_clean", "{}"),
            refused("refused", "agents_clean", UNCONFIRMED),
            write("apply", "agents_clean", APPLY),
        ],
    ),
    case(
        "agents_uninstall",
        &[
            setup("agents_sync", CLAUDE_APPLY),
            read("preview", "agents_uninstall", r#"{"name":"helper"}"#),
            refused(
                "refused",
                "agents_uninstall",
                r#"{"name":"helper","dry_run":false}"#,
            ),
            fails(
                "escaping_name",
                "invalid_arguments",
                "agents_uninstall",
                r#"{"name":"../skills/demo","dry_run":false,"confirm":true}"#,
            ),
            write(
                "apply",
                "agents_uninstall",
                r#"{"name":"helper","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "agents_edit_body",
        &[
            refused(
                "refused",
                "agents_edit_body",
                r#"{"name":"helper","new_body":"Changed prompt\n"}"#,
            ),
            write(
                "apply",
                "agents_edit_body",
                r#"{"name":"helper","new_body":"Changed prompt\n","confirm":true}"#,
            ),
        ],
    ),
    // styles
    case("styles_list", &[read("", "styles_list", "{}")]),
    case(
        "styles_get",
        &[
            read("found", "styles_get", r#"{"name":"plain"}"#),
            fails("missing", "not_found", "styles_get", r#"{"name":"nope"}"#),
        ],
    ),
    case("styles_lint", &[read("", "styles_lint", "{}")]),
    case(
        "styles_active",
        &[
            read("none", "styles_active", "{}"),
            setup("styles_apply", AIDER),
            read("applied", "styles_active", "{}"),
        ],
    ),
    case(
        "styles_list_transpilers",
        &[read("", "styles_list_transpilers", "{}")],
    ),
    case(
        "styles_scaffold",
        &[
            write("apply", "styles_scaffold", r#"{"name":"terse"}"#),
            fails(
                "exists",
                "conflict",
                "styles_scaffold",
                r#"{"name":"terse"}"#,
            ),
        ],
    ),
    case(
        "styles_sync_tier1",
        &[
            read("preview", "styles_sync_tier1", CLAUDE),
            refused("refused", "styles_sync_tier1", CLAUDE_UNCONFIRMED),
            write("apply", "styles_sync_tier1", CLAUDE_APPLY),
        ],
    ),
    case(
        "styles_apply",
        &[
            read("preview", "styles_apply", AIDER_STYLE),
            refused("refused", "styles_apply", AIDER_STYLE_UNCONFIRMED),
            write("apply", "styles_apply", AIDER),
            fails(
                "missing",
                "not_found",
                "styles_apply",
                r#"{"name":"nope","client_keys":["aider"],"dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "styles_diff",
        &[
            read("fresh", "styles_diff", "{}"),
            setup("styles_sync_tier1", CLAUDE_APPLY),
            read("synced", "styles_diff", "{}"),
        ],
    ),
    case(
        "styles_status",
        &[
            read("fresh", "styles_status", "{}"),
            setup("styles_sync_tier1", CLAUDE_APPLY),
            read("synced", "styles_status", "{}"),
        ],
    ),
    case(
        "styles_clean",
        &[
            setup("styles_sync_tier1", CLAUDE_APPLY),
            read("preview", "styles_clean", "{}"),
            refused("refused", "styles_clean", UNCONFIRMED),
            write("apply", "styles_clean", APPLY),
        ],
    ),
    case(
        "styles_edit_body",
        &[
            refused(
                "refused",
                "styles_edit_body",
                r#"{"name":"plain","new_body":"Short\n"}"#,
            ),
            write(
                "apply",
                "styles_edit_body",
                r#"{"name":"plain","new_body":"Short\n","confirm":true}"#,
            ),
        ],
    ),
    case(
        "styles_remove",
        &[
            setup("styles_apply", AIDER),
            read("preview", "styles_remove", AIDER_REMOVE_PREVIEW),
            refused("refused", "styles_remove", AIDER_REMOVE_UNCONFIRMED),
            write("apply", "styles_remove", AIDER_REMOVE),
        ],
    ),
    case(
        "catalog_status",
        &[
            read("empty", "catalog_status", "{}"),
            hook("stopped-world", catalog_world::stopped),
            read("stopped", "catalog_status", "{}"),
        ],
    ),
    prepared(
        "library_status",
        library_home,
        &[
            hook("ahead-one-world", library_world::ahead_one),
            read("clone", "library_status", "{}"),
            read("fetch", "library_status", r#"{"fetch":true}"#),
        ],
    ),
    prepared(
        "library_pull",
        library_home,
        &[
            hook("behind-two-world", library_world::behind_two),
            hook("dirty-world", library_world::dirty),
            fails("dirty", "refused", "library_pull", "{}"),
            fails(
                "dirty_apply",
                "refused",
                "library_pull",
                r#"{"dry_run":false}"#,
            ),
            hook("clean-world", library_world::clean),
            read("preview", "library_pull", "{}"),
            write("apply", "library_pull", r#"{"dry_run":false}"#),
            read("current", "library_pull", "{}"),
        ],
    ),
    prepared(
        "library_connect",
        library_world::remote_only,
        &[
            read(
                "preview",
                "library_connect",
                r#"{"url":"{base}/lib-remote.git","clone_to":"{home}/lib/ai-skills"}"#,
            ),
            refused(
                "refused",
                "library_connect",
                r#"{"url":"{base}/lib-remote.git","clone_to":"{home}/lib/ai-skills","dry_run":false}"#,
            ),
            write(
                "apply",
                "library_connect",
                r#"{"url":"{base}/lib-remote.git","clone_to":"{home}/lib/ai-skills","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "again",
                "refused",
                "library_connect",
                r#"{"url":"{base}/lib-remote.git","clone_to":"{home}/lib/other"}"#,
            ),
            fails(
                "credential",
                "invalid_arguments",
                "library_connect",
                r#"{"url":"https://user:pass@example.invalid/o/skills.git"}"#,
            ),
        ],
    ),
    prepared(
        "library_remote_add",
        library_home,
        &[
            hook("no-remote-world", library_world::no_remote),
            read(
                "preview",
                "library_remote_add",
                r#"{"url":"{base}/lib-remote.git"}"#,
            ),
            refused(
                "refused",
                "library_remote_add",
                r#"{"url":"{base}/lib-remote.git","dry_run":false}"#,
            ),
            write(
                "apply",
                "library_remote_add",
                r#"{"url":"{base}/lib-remote.git","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "again",
                "refused",
                "library_remote_add",
                r#"{"url":"{base}/lib-remote.git"}"#,
            ),
        ],
    ),
    prepared(
        "library_commit",
        library_home,
        &[
            read("clean", "library_commit", r#"{"message":"Nothing to do"}"#),
            hook("dirty-world", library_world::dirty),
            read(
                "preview",
                "library_commit",
                r#"{"message":"Update skills"}"#,
            ),
            refused(
                "refused",
                "library_commit",
                r#"{"message":"Update skills","dry_run":false}"#,
            ),
            write(
                "apply",
                "library_commit",
                r#"{"message":"Update skills","dry_run":false,"confirm":true}"#,
            ),
            hook("secret-world", library_world::secret_dirty),
            fails(
                "secret",
                "refused",
                "library_commit",
                r#"{"message":"Add leaky","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "no_message",
                "invalid_arguments",
                "library_commit",
                r#"{"message":" ","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    prepared(
        "skills_git_push",
        library_world::skills_repo_remote_world,
        &[
            read(
                "preview",
                "skills_git_push",
                r#"{"commit_message":"add extra"}"#,
            ),
            read("preview_without_message", "skills_git_push", "{}"),
            read(
                "confirmed_preview",
                "skills_git_push",
                r#"{"commit_message":"add extra","confirm":true}"#,
            ),
            refused(
                "refused",
                "skills_git_push",
                r#"{"commit_message":"add extra","dry_run":false}"#,
            ),
            fails(
                "empty_message",
                "invalid_arguments",
                "skills_git_push",
                r#"{"commit_message":" ","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "not_a_repository",
                "not_found",
                "skills_git_push",
                r#"{"commit_message":"x","repo_path":"{data}","dry_run":false,"confirm":true}"#,
            ),
            write(
                "apply",
                "skills_git_push",
                r#"{"commit_message":"add extra","dry_run":false,"confirm":true}"#,
            ),
            read(
                "clean",
                "skills_git_push",
                r#"{"commit_message":"again","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case("flow_diagram", &[read("", "flow_diagram", "{}")]),
    case(
        "servers_list",
        &[
            read("default", "servers_list", "{}"),
            read("sources", "servers_list", r#"{"include_sources":true}"#),
        ],
    ),
    case(
        "servers_get",
        &[
            read("found", "servers_get", r#"{"name":"alpha"}"#),
            fails("missing", "not_found", "servers_get", r#"{"name":"nope"}"#),
        ],
    ),
    case(
        "servers_list_profiles",
        &[read("", "servers_list_profiles", "{}")],
    ),
    case(
        "servers_add_profile_tag",
        &[
            write(
                "apply",
                "servers_add_profile_tag",
                r#"{"name":"beta","profile_tag":"Default"}"#,
            ),
            fails(
                "missing",
                "not_found",
                "servers_add_profile_tag",
                r#"{"name":"nope","profile_tag":"Default"}"#,
            ),
        ],
    ),
    case(
        "servers_remove_profile_tag",
        &[
            setup(
                "servers_add_profile_tag",
                r#"{"name":"beta","profile_tag":"Default"}"#,
            ),
            write(
                "apply",
                "servers_remove_profile_tag",
                r#"{"name":"beta","profile_tag":"Default"}"#,
            ),
        ],
    ),
    case(
        "servers_install",
        &[
            refused("refused", "servers_install", GAMMA),
            write("apply", "servers_install", GAMMA_APPLY),
            fails("exists", "conflict", "servers_install", GAMMA_APPLY),
        ],
    ),
    case(
        "servers_update_config",
        &[
            setup("servers_install", GAMMA_APPLY),
            refused(
                "refused",
                "servers_update_config",
                r#"{"name":"gamma","patch":{"args":["--changed"]}}"#,
            ),
            write(
                "apply",
                "servers_update_config",
                r#"{"name":"gamma","patch":{"args":["--changed"]},"confirm":true}"#,
            ),
            write(
                "env",
                "servers_update_config",
                r#"{"name":"gamma","patch":{"env":{"JAVA_HOME":"/opt/jdk","MODE":"fast"}},"confirm":true}"#,
            ),
            fails(
                "env-refused",
                "invalid_arguments",
                "servers_update_config",
                r#"{"name":"gamma","patch":{"env":{"API_KEY":"plain"}},"confirm":true}"#,
            ),
            fails(
                "missing",
                "not_found",
                "servers_update_config",
                r#"{"name":"nope","patch":{"args":[]},"confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_set_mode",
        &[
            refused(
                "refused",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"search"}"#,
            ),
            fails(
                "unknown_mode",
                "invalid_arguments",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"bogus","confirm":true}"#,
            ),
            fails(
                "retired_mode",
                "invalid_arguments",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"direct","confirm":true}"#,
            ),
            fails(
                "missing",
                "not_found",
                "servers_set_mode",
                r#"{"name":"nope","mode":"search","confirm":true}"#,
            ),
            write(
                "apply",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"search","confirm":true}"#,
            ),
            write(
                "back_to_auto",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"auto","confirm":true}"#,
            ),
        ],
    ),
    prepared(
        "access_set_ls",
        folder_routing_world,
        &[read("ls", "access_set_ls", "{}")],
    ),
    prepared(
        "access_set_add",
        folder_routing_world,
        &[
            read(
                "plan",
                "access_set_add",
                r#"{"name":"Deep Work","servers":["beta"]}"#,
            ),
            refused(
                "refused",
                "access_set_add",
                r#"{"name":"Deep Work","dry_run":false}"#,
            ),
            write(
                "apply",
                "access_set_add",
                r#"{"name":"Deep Work","servers":["beta"],"dry_run":false,"confirm":true}"#,
            ),
            fails(
                "exists",
                "conflict",
                "access_set_add",
                r#"{"name":"deep work"}"#,
            ),
            fails(
                "unknown_server",
                "not_found",
                "access_set_add",
                r#"{"name":"New","servers":["nope"]}"#,
            ),
        ],
    ),
    prepared(
        "access_set_rm",
        folder_routing_world,
        &[
            fails("in_use", "conflict", "access_set_rm", r#"{"set":"wide"}"#),
            read(
                "detach_plan",
                "access_set_rm",
                r#"{"set":"wide","orphans":"detach"}"#,
            ),
            refused(
                "refused",
                "access_set_rm",
                r#"{"set":"wide","orphans":"detach","dry_run":false}"#,
            ),
            write(
                "apply",
                "access_set_rm",
                r#"{"set":"wide","orphans":"detach","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "default",
                "conflict",
                "access_set_rm",
                r#"{"set":"default"}"#,
            ),
            fails(
                "bad_orphans",
                "invalid_arguments",
                "access_set_rm",
                r#"{"set":"focus","orphans":"drop"}"#,
            ),
        ],
    ),
    prepared(
        "access_set_default",
        folder_routing_world,
        &[
            read("plan", "access_set_default", r#"{"set":"wide"}"#),
            refused(
                "refused",
                "access_set_default",
                r#"{"set":"wide","dry_run":false}"#,
            ),
            write(
                "apply",
                "access_set_default",
                r#"{"set":"wide","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "unknown_set",
                "not_found",
                "access_set_default",
                r#"{"set":"nope"}"#,
            ),
            fails("nothing", "invalid_arguments", "access_set_default", "{}"),
        ],
    ),
    prepared(
        "access_set_tools",
        folder_routing_world,
        &[
            read("list", "access_set_tools", "{}"),
            read(
                "preview",
                "access_set_tools",
                r#"{"set":"wide","server":"alpha","tools":["read","list"]}"#,
            ),
            refused(
                "refused",
                "access_set_tools",
                r#"{"set":"wide","server":"alpha","tools":["read"],"dry_run":false}"#,
            ),
            write(
                "apply",
                "access_set_tools",
                r#"{"set":"wide","server":"alpha","tools":["read","list"],"dry_run":false,"confirm":true}"#,
            ),
            read("narrowed", "access_set_tools", r#"{"set":"wide"}"#),
            write(
                "restore",
                "access_set_tools",
                r#"{"set":"wide","server":"alpha","all":true,"dry_run":false,"confirm":true}"#,
            ),
            fails(
                "not_listed",
                "conflict",
                "access_set_tools",
                r#"{"set":"focus","server":"beta","all":true}"#,
            ),
            fails(
                "unknown_set",
                "not_found",
                "access_set_tools",
                r#"{"set":"nope","server":"alpha","all":true}"#,
            ),
            fails(
                "unknown_server",
                "not_found",
                "access_set_tools",
                r#"{"set":"wide","server":"nope","all":true}"#,
            ),
            fails(
                "both",
                "invalid_arguments",
                "access_set_tools",
                r#"{"set":"wide","server":"alpha","tools":["x"],"all":true}"#,
            ),
            fails(
                "neither",
                "invalid_arguments",
                "access_set_tools",
                r#"{"set":"wide","server":"alpha"}"#,
            ),
            fails(
                "no_server",
                "invalid_arguments",
                "access_set_tools",
                r#"{"set":"wide","tools":["x"]}"#,
            ),
            fails(
                "blank_name",
                "invalid_arguments",
                "access_set_tools",
                r#"{"set":"wide","server":"alpha","tools":[" "]}"#,
            ),
        ],
    ),
    prepared(
        "client_scope",
        folder_routing_world,
        &[
            read(
                "plan",
                "client_scope",
                r#"{"client":"claude-code","set":"wide"}"#,
            ),
            refused(
                "refused",
                "client_scope",
                r#"{"client":"claude-code","none":true,"dry_run":false}"#,
            ),
            write(
                "apply",
                "client_scope",
                r#"{"client":"claude-code","none":true,"dry_run":false,"confirm":true}"#,
            ),
            fails(
                "unknown_client",
                "not_found",
                "client_scope",
                r#"{"client":"claud-code","set":"wide"}"#,
            ),
        ],
    ),
    prepared(
        "folder_route_ls",
        folder_routing_world,
        &[read("ls", "folder_route_ls", "{}")],
    ),
    prepared(
        "folder_route_add",
        folder_routing_world,
        &[
            read(
                "plan",
                "folder_route_add",
                r#"{"folder":"/work/other","set":"focus"}"#,
            ),
            write(
                "apply",
                "folder_route_add",
                r#"{"folder":"/work/other","set":"focus","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "relative",
                "invalid_arguments",
                "folder_route_add",
                r#"{"folder":"other","set":"focus"}"#,
            ),
            fails(
                "unknown_set",
                "not_found",
                "folder_route_add",
                r#"{"folder":"/work/other","set":"nope"}"#,
            ),
        ],
    ),
    prepared(
        "folder_route_rm",
        folder_routing_world,
        &[
            read("plan", "folder_route_rm", r#"{"folder":"/work/proj"}"#),
            refused(
                "refused",
                "folder_route_rm",
                r#"{"folder":"/work/proj","dry_run":false}"#,
            ),
            write(
                "apply",
                "folder_route_rm",
                r#"{"folder":"/work/proj","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "gone",
                "not_found",
                "folder_route_rm",
                r#"{"folder":"/work/proj"}"#,
            ),
        ],
    ),
    prepared(
        "clients_reach",
        folder_routing_world,
        &[
            read("folder_outranks", "clients_reach", "{}"),
            read("one_server", "clients_reach", r#"{"name":"beta"}"#),
            fails(
                "unknown_server",
                "not_found",
                "clients_reach",
                r#"{"name":"nope"}"#,
            ),
        ],
    ),
    prepared(
        "client_direct_ls",
        claude_code_home,
        &[
            read("empty", "client_direct_ls", "{}"),
            setup(
                "client_direct_add",
                r#"{"name":"alpha","client":"claude-code","dry_run":false,"confirm":true}"#,
            ),
            read("entries", "client_direct_ls", r#"{"client":"claude-code"}"#),
        ],
    ),
    prepared(
        "client_direct_check",
        claude_code_home,
        &[
            setup(
                "client_direct_add",
                r#"{"name":"alpha","client":"claude-code","dry_run":false,"confirm":true}"#,
            ),
            read("in_sync", "client_direct_check", "{}"),
        ],
    ),
    prepared(
        "client_direct_add",
        claude_code_home,
        &[
            read(
                "plan",
                "client_direct_add",
                r#"{"name":"alpha","client":"claude-code"}"#,
            ),
            refused(
                "refused",
                "client_direct_add",
                r#"{"name":"alpha","client":"claude-code","dry_run":false}"#,
            ),
            write(
                "apply",
                "client_direct_add",
                r#"{"name":"alpha","client":"claude-code","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "unsupported",
                "backend_error",
                "client_direct_add",
                r#"{"name":"alpha","client":"goose"}"#,
            ),
            fails(
                "missing",
                "not_found",
                "client_direct_add",
                r#"{"name":"nope","client":"claude-code"}"#,
            ),
        ],
    ),
    prepared(
        "client_direct_rm",
        claude_code_home,
        &[
            setup(
                "client_direct_add",
                r#"{"name":"alpha","client":"claude-code","dry_run":false,"confirm":true}"#,
            ),
            read(
                "plan",
                "client_direct_rm",
                r#"{"name":"alpha","client":"claude-code"}"#,
            ),
            refused(
                "refused",
                "client_direct_rm",
                r#"{"name":"alpha","client":"claude-code","dry_run":false}"#,
            ),
            write(
                "apply",
                "client_direct_rm",
                r#"{"name":"alpha","client":"claude-code","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_uninstall",
        &[
            read("preview", "servers_uninstall", r#"{"name":"beta"}"#),
            refused(
                "refused",
                "servers_uninstall",
                r#"{"name":"beta","dry_run":false}"#,
            ),
            read(
                "confirmed_preview",
                "servers_uninstall",
                r#"{"name":"beta","confirm":true}"#,
            ),
            write(
                "apply",
                "servers_uninstall",
                r#"{"name":"beta","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "gone",
                "not_found",
                "servers_uninstall",
                r#"{"name":"beta","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_auth",
        &[
            refused("refused", "servers_auth", r#"{"name":"alpha"}"#),
            read(
                "apply",
                "servers_auth",
                r#"{"name":"alpha","confirm":true}"#,
            ),
            fails(
                "unknown",
                "not_found",
                "servers_auth",
                r#"{"name":"ghost","confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_set_enabled",
        &[
            read(
                "preview",
                "servers_set_enabled",
                r#"{"name":"alpha","enabled":false}"#,
            ),
            refused(
                "refused",
                "servers_set_enabled",
                r#"{"name":"alpha","enabled":false,"dry_run":false}"#,
            ),
            read(
                "unchanged",
                "servers_set_enabled",
                r#"{"name":"alpha","enabled":true,"dry_run":false,"confirm":true}"#,
            ),
            write(
                "off",
                "servers_set_enabled",
                r#"{"name":"alpha","enabled":false,"dry_run":false,"confirm":true}"#,
            ),
            write(
                "on",
                "servers_set_enabled",
                r#"{"name":"alpha","enabled":true,"dry_run":false,"confirm":true}"#,
            ),
            fails(
                "unknown",
                "not_found",
                "servers_set_enabled",
                r#"{"name":"ghost","enabled":true}"#,
            ),
            fails(
                "no_value",
                "invalid_arguments",
                "servers_set_enabled",
                r#"{"name":"alpha"}"#,
            ),
        ],
    ),
    case(
        "client_discovery_set",
        &[
            read(
                "preview",
                "client_discovery_set",
                r#"{"client":"claude-code","mode":"lazy"}"#,
            ),
            refused(
                "refused",
                "client_discovery_set",
                r#"{"client":"claude-code","mode":"lazy","dry_run":false}"#,
            ),
            write(
                "pin",
                "client_discovery_set",
                r#"{"client":"claude-code","mode":"grouped","dry_run":false,"confirm":true}"#,
            ),
            read(
                "unchanged",
                "client_discovery_set",
                r#"{"client":"claude-code","mode":"grouped","dry_run":false,"confirm":true}"#,
            ),
            write(
                "inherit",
                "client_discovery_set",
                r#"{"client":"claude-code","mode":"inherit","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "bad_mode",
                "invalid_arguments",
                "client_discovery_set",
                r#"{"client":"claude-code","mode":"auto"}"#,
            ),
            fails(
                "no_mode",
                "invalid_arguments",
                "client_discovery_set",
                r#"{"client":"claude-code"}"#,
            ),
            fails(
                "unknown_client",
                "not_found",
                "client_discovery_set",
                r#"{"client":"claud-code","mode":"full"}"#,
            ),
        ],
    ),
    case(
        "servers_restart",
        &[
            read("preview", "servers_restart", r#"{"name":"alpha"}"#),
            refused(
                "refused",
                "servers_restart",
                r#"{"name":"alpha","dry_run":false}"#,
            ),
        ],
    ),
    prepared(
        "servers_repo_status",
        server_repo,
        &[
            read("behind", "servers_repo_status", "{}"),
            read("one", "servers_repo_status", r#"{"name":"gamma"}"#),
            fails(
                "no_checkout",
                "not_found",
                "servers_repo_status",
                r#"{"name":"alpha"}"#,
            ),
        ],
    ),
    prepared(
        "servers_repo_check",
        server_repo,
        &[write("fetched", "servers_repo_check", "{}")],
    ),
    prepared(
        "servers_repo_update",
        server_repo,
        &[
            setup("servers_repo_check", "{}"),
            read("plan", "servers_repo_update", r#"{"name":"gamma"}"#),
            read(
                "plan_build",
                "servers_repo_update",
                r#"{"name":"gamma","build":"echo built"}"#,
            ),
            fails(
                "needs_hash",
                "invalid_arguments",
                "servers_repo_update",
                r#"{"name":"gamma","dry_run":false,"confirm":true}"#,
            ),
            refused(
                "refused",
                "servers_repo_update",
                r#"{"name":"gamma","dry_run":false,"plan_hash":"sha256:15f1e35a8e23a6ff"}"#,
            ),
            fails(
                "stale_hash",
                "conflict",
                "servers_repo_update",
                r#"{"name":"gamma","build":"echo built","dry_run":false,"plan_hash":"000000000000","confirm":true}"#,
            ),
            fails(
                "other_build",
                "conflict",
                "servers_repo_update",
                r#"{"name":"gamma","build":"echo other","dry_run":false,"plan_hash":"sha256:15f1e35a8e23a6ff","confirm":true}"#,
            ),
            fails(
                "unknown",
                "not_found",
                "servers_repo_update",
                r#"{"name":"ghost"}"#,
            ),
            write(
                "update",
                "servers_repo_update",
                r#"{"name":"gamma","build":"echo built","dry_run":false,"plan_hash":"sha256:15f1e35a8e23a6ff","confirm":true}"#,
            ),
            read("up_to_date", "servers_repo_update", r#"{"name":"gamma"}"#),
            hook("diverge", server_repo_diverged),
            setup_as("check_again", "servers_repo_check", "{}"),
            read(
                "diverged_plan",
                "servers_repo_update",
                r#"{"name":"gamma"}"#,
            ),
            fails(
                "diverged",
                "backend_error",
                "servers_repo_update",
                r#"{"name":"gamma","dry_run":false,"plan_hash":"sha256:15f1e35a8e23a6ff","confirm":true}"#,
            ),
        ],
    ),
    prepared(
        "servers_repo_switch",
        server_repo_with_peers,
        &[
            read("plan_cached", "servers_repo_switch", r#"{"name":"gamma"}"#),
            read(
                "plan",
                "servers_repo_switch",
                r#"{"name":"gamma","fetch":true}"#,
            ),
            fails(
                "needs_hash",
                "invalid_arguments",
                "servers_repo_switch",
                r#"{"name":"gamma","dry_run":false,"confirm":true}"#,
            ),
            refused(
                "refused",
                "servers_repo_switch",
                r#"{"name":"gamma","dry_run":false,"plan_hash":"sha256:7772a977fa8898ee"}"#,
            ),
            fails(
                "stale_hash",
                "conflict",
                "servers_repo_switch",
                r#"{"name":"gamma","fetch":true,"dry_run":false,"plan_hash":"000000000000","confirm":true}"#,
            ),
            fails(
                "unknown_machine",
                "not_found",
                "servers_repo_switch",
                r#"{"name":"gamma","machine":"ghost"}"#,
            ),
            write(
                "switch",
                "servers_repo_switch",
                r#"{"name":"gamma","fetch":true,"dry_run":false,"plan_hash":"sha256:7772a977fa8898ee","confirm":true}"#,
            ),
            setup_as(
                "undo_switch",
                "servers_repo_undo",
                r#"{"name":"gamma","of":"switch","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    prepared(
        "servers_repo_undo",
        server_repo,
        &[
            setup("servers_repo_check", "{}"),
            fails(
                "nothing",
                "not_found",
                "servers_repo_undo",
                r#"{"name":"gamma"}"#,
            ),
            setup(
                "servers_repo_update",
                r#"{"name":"gamma","build":"echo built","dry_run":false,"plan_hash":"sha256:15f1e35a8e23a6ff","confirm":true}"#,
            ),
            read("preview", "servers_repo_undo", r#"{"name":"gamma"}"#),
            refused(
                "refused",
                "servers_repo_undo",
                r#"{"name":"gamma","dry_run":false}"#,
            ),
            fails(
                "bad_of",
                "invalid_arguments",
                "servers_repo_undo",
                r#"{"name":"gamma","of":"both"}"#,
            ),
            fails(
                "no_switch",
                "not_found",
                "servers_repo_undo",
                r#"{"name":"gamma","of":"switch"}"#,
            ),
            write(
                "undo",
                "servers_repo_undo",
                r#"{"name":"gamma","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "again",
                "not_found",
                "servers_repo_undo",
                r#"{"name":"gamma","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    context_case("context_bundle_ls", &[read("", "context_bundle_ls", "{}")]),
    context_case(
        "context_bundle_status",
        &[
            read("none", "context_bundle_status", r#"{"cwd":"{home}/proj"}"#),
            fails("no-cwd", "invalid_arguments", "context_bundle_status", "{}"),
            fails(
                "bad-cwd",
                "invalid_arguments",
                "context_bundle_status",
                r#"{"cwd":"{home}/gone"}"#,
            ),
        ],
    ),
    context_case(
        "context_bundle_show",
        &[
            read("bundle", "context_bundle_show", r#"{"name":"acme-dev"}"#),
            fails(
                "missing",
                "not_found",
                "context_bundle_show",
                r#"{"name":"nope"}"#,
            ),
            fails("no-name", "invalid_arguments", "context_bundle_show", "{}"),
        ],
    ),
    context_case(
        "context_bundle_config",
        &[
            read("current", "context_bundle_config", "{}"),
            read("preview", "context_bundle_config", r#"{"auto_apply":"on"}"#),
            refused(
                "refused",
                "context_bundle_config",
                r#"{"auto_apply":"on","dry_run":false}"#,
            ),
            fails(
                "bad_value",
                "invalid_arguments",
                "context_bundle_config",
                r#"{"auto_apply":"yes"}"#,
            ),
            write(
                "apply",
                "context_bundle_config",
                r#"{"auto_apply":"on","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    context_case(
        "context_sync",
        &[
            read("preview", "context_sync", "{}"),
            refused("refused", "context_sync", UNCONFIRMED),
            write("apply", "context_sync", APPLY),
            setup(
                "context_bundle_config",
                r#"{"auto_apply":"on","dry_run":false,"confirm":true}"#,
            ),
            setup(
                "context_bundle_apply",
                r#"{"name":"acme-dev","cwd":"{home}/proj"}"#,
            ),
            read("with_bundles", "context_sync", "{}"),
        ],
    ),
    context_case(
        "context_bundle_apply",
        &[
            read(
                "plan",
                "context_bundle_apply",
                r#"{"name":"acme-dev","cwd":"{home}/proj","dry_run":true}"#,
            ),
            write(
                "result",
                "context_bundle_apply",
                r#"{"name":"acme-dev","cwd":"{home}/proj"}"#,
            ),
            fails(
                "missing",
                "not_found",
                "context_bundle_apply",
                r#"{"name":"nope","cwd":"{home}/proj"}"#,
            ),
            fails(
                "no-name",
                "invalid_arguments",
                "context_bundle_apply",
                r#"{"cwd":"{home}/proj"}"#,
            ),
        ],
    ),
    context_case(
        "context_bundle_undo",
        &[
            fails(
                "none",
                "conflict",
                "context_bundle_undo",
                r#"{"cwd":"{home}/proj"}"#,
            ),
            setup(
                "context_bundle_apply",
                r#"{"name":"acme-dev","cwd":"{home}/proj"}"#,
            ),
            read(
                "plan",
                "context_bundle_undo",
                r#"{"cwd":"{home}/proj","dry_run":true}"#,
            ),
            write("result", "context_bundle_undo", r#"{"cwd":"{home}/proj"}"#),
        ],
    ),
    prepared(
        "context_launch_profile_ls",
        launch_profile_world,
        &[
            read("none", "context_launch_profile_ls", "{}"),
            setup(
                "context_launch_profile_add",
                r#"{"name":"bare","set":"bare","dry_run":false,"confirm":true}"#,
            ),
            read("one", "context_launch_profile_ls", r#"{"name":"bare"}"#),
            fails(
                "unknown",
                "not_found",
                "context_launch_profile_ls",
                r#"{"name":"nope"}"#,
            ),
        ],
    ),
    prepared(
        "context_launch_profile_add",
        launch_profile_world,
        &[
            read(
                "plan",
                "context_launch_profile_add",
                r#"{"name":"bare","set":"bare"}"#,
            ),
            refused(
                "refused",
                "context_launch_profile_add",
                r#"{"name":"bare","set":"bare","dry_run":false}"#,
            ),
            write(
                "apply",
                "context_launch_profile_add",
                r#"{"name":"bare","set":"bare","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "unknown_set",
                "not_found",
                "context_launch_profile_add",
                r#"{"name":"other","set":"nope"}"#,
            ),
        ],
    ),
    prepared(
        "context_launch_profile_rm",
        launch_profile_world,
        &[
            setup(
                "context_launch_profile_add",
                r#"{"name":"bare","set":"bare","dry_run":false,"confirm":true}"#,
            ),
            read("plan", "context_launch_profile_rm", r#"{"name":"bare"}"#),
            write(
                "apply",
                "context_launch_profile_rm",
                r#"{"name":"bare","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "gone",
                "not_found",
                "context_launch_profile_rm",
                r#"{"name":"bare"}"#,
            ),
        ],
    ),
    prepared(
        "context_launch_profile_policy",
        launch_profile_world,
        &[
            setup(
                "client_direct_add",
                r#"{"name":"alpha","client":"claude-code","dry_run":false,"confirm":true}"#,
            ),
            setup(
                "context_launch_profile_add",
                r#"{"name":"bare","set":"empty","deny_direct":true,"dry_run":false,"confirm":true}"#,
            ),
            read(
                "plan",
                "context_launch_profile_policy",
                r#"{"name":"bare","cwd":"{home}/proj"}"#,
            ),
            write(
                "apply",
                "context_launch_profile_policy",
                r#"{"name":"bare","cwd":"{home}/proj","dry_run":false}"#,
            ),
            fails(
                "unknown",
                "not_found",
                "context_launch_profile_policy",
                r#"{"name":"nope","cwd":"{home}/proj"}"#,
            ),
        ],
    ),
    context_case(
        "context_leave_out",
        &[
            read(
                "plan",
                "context_leave_out",
                r#"{"action":"add","path":"{home}/.claude/CLAUDE.md","cwd":"{home}/proj"}"#,
            ),
            write(
                "apply",
                "context_leave_out",
                r#"{"action":"add","path":"{home}/.claude/CLAUDE.md","cwd":"{home}/proj","dry_run":false}"#,
            ),
            write(
                "memory",
                "context_leave_out",
                r#"{"action":"add","memory":true,"cwd":"{home}/proj","dry_run":false}"#,
            ),
            fails(
                "bad-action",
                "invalid_arguments",
                "context_leave_out",
                r#"{"action":"drop","memory":true,"cwd":"{home}/proj"}"#,
            ),
            fails(
                "both-targets",
                "invalid_arguments",
                "context_leave_out",
                r#"{"action":"add","memory":true,"path":"{home}/x","cwd":"{home}/proj"}"#,
            ),
        ],
    ),
    prepared(
        "memory_ls",
        World::memory_files,
        &[
            read("folder", "memory_ls", r#"{"cwd":"{home}/proj-wt"}"#),
            read("all", "memory_ls", r#"{"all":true}"#),
            fails("no-cwd", "invalid_arguments", "memory_ls", "{}"),
        ],
    ),
    prepared(
        "memory_status",
        World::memory_files,
        &[
            read("folder", "memory_status", r#"{"cwd":"{home}/proj"}"#),
            read("by-hand", "memory_status", r#"{"cwd":"{home}/other"}"#),
            fails("no-cwd", "invalid_arguments", "memory_status", "{}"),
        ],
    ),
    prepared(
        "memory_set",
        World::memory_files,
        &[
            read(
                "preview",
                "memory_set",
                r#"{"position":"off","cwd":"{home}/proj-wt"}"#,
            ),
            write(
                "apply",
                "memory_set",
                r#"{"position":"off","cwd":"{home}/proj-wt","dry_run":false}"#,
            ),
            read(
                "global-preview",
                "memory_set",
                r#"{"position":"off","global":true}"#,
            ),
            fails(
                "bad-position",
                "invalid_arguments",
                "memory_set",
                r#"{"position":"maybe","cwd":"{home}/proj"}"#,
            ),
            fails(
                "both",
                "invalid_arguments",
                "memory_set",
                r#"{"position":"on","cwd":"{home}/proj","global":true}"#,
            ),
        ],
    ),
    prepared(
        "memory_clear",
        World::memory_files,
        &[
            read("plan", "memory_clear", r#"{"cwd":"{home}/proj-wt"}"#),
            read(
                "plan-with-off",
                "memory_clear",
                r#"{"cwd":"{home}/proj","also_off":true}"#,
            ),
            fails("no-cwd", "invalid_arguments", "memory_clear", "{}"),
        ],
    ),
    prepared(
        "memory_undo",
        World::trashed_files,
        &[
            read("preview", "memory_undo", r#"{"cwd":"{home}/proj"}"#),
            write(
                "apply",
                "memory_undo",
                r#"{"id":"memory-20260101-000000","dry_run":false}"#,
            ),
            fails(
                "nothing",
                "not_found",
                "memory_undo",
                r#"{"cwd":"{home}/proj"}"#,
            ),
            fails(
                "wrong-kind",
                "invalid_arguments",
                "memory_undo",
                r#"{"id":"sessions-20260101-000000"}"#,
            ),
        ],
    ),
    prepared(
        "memory_overview",
        World::memory_files,
        &[
            read("all", "memory_overview", r#"{"all":true}"#),
            read(
                "filtered",
                "memory_overview",
                r#"{"all":true,"filter":"memory-off","sort":"size"}"#,
            ),
            read("folder", "memory_overview", r#"{"cwd":"{home}/proj"}"#),
            fails("no-cwd", "invalid_arguments", "memory_overview", "{}"),
        ],
    ),
    prepared(
        "sessions_ls",
        World::memory_files,
        &[
            read("folder", "sessions_ls", r#"{"cwd":"{home}/proj-wt"}"#),
            read("all", "sessions_ls", r#"{"all":true}"#),
            fails("no-cwd", "invalid_arguments", "sessions_ls", "{}"),
        ],
    ),
    prepared(
        "sessions_clear",
        World::memory_files,
        &[
            read(
                "plan",
                "sessions_clear",
                r#"{"cwd":"{home}/proj","older_than_days":7}"#,
            ),
            read(
                "plan-ids",
                "sessions_clear",
                r#"{"cwd":"{home}/proj","ids":["sess-new"],"to":"archive"}"#,
            ),
            fails(
                "no-selection",
                "invalid_arguments",
                "sessions_clear",
                r#"{"cwd":"{home}/proj"}"#,
            ),
        ],
    ),
    prepared(
        "sessions_undo",
        World::trashed_files,
        &[
            read("preview", "sessions_undo", r#"{"cwd":"{home}/proj"}"#),
            write(
                "apply",
                "sessions_undo",
                r#"{"id":"sessions-20260101-000000","dry_run":false}"#,
            ),
            fails(
                "nothing",
                "not_found",
                "sessions_undo",
                r#"{"cwd":"{home}/proj"}"#,
            ),
        ],
    ),
    prepared(
        "sessions_retention",
        World::memory_files,
        &[read("current", "sessions_retention", "{}")],
    ),
    context_case(
        "context_loads",
        &[
            read("folder", "context_loads", r#"{"cwd":"{home}/proj"}"#),
            read(
                "measured",
                "context_loads",
                r#"{"cwd":"{home}/proj","measured":true}"#,
            ),
            fails("no-cwd", "invalid_arguments", "context_loads", "{}"),
        ],
    ),
    context_case(
        "context_compose",
        &[
            read("folder", "context_compose", r#"{"cwd":"{home}/proj"}"#),
            fails(
                "bad-cwd",
                "invalid_arguments",
                "context_compose",
                r#"{"cwd":"{home}/gone"}"#,
            ),
        ],
    ),
    context_case(
        "context_measure",
        &[
            read("plan", "context_measure", r#"{"cwd":"{home}/proj"}"#),
            read(
                "variants",
                "context_measure",
                r#"{"cwd":"{home}/proj","without":["plugin:kit@market"],"bundle":"acme-dev"}"#,
            ),
            fails("no-cwd", "invalid_arguments", "context_measure", "{}"),
        ],
    ),
    context_case(
        "context_status",
        &[
            read("folder", "context_status", r#"{"cwd":"{home}/proj"}"#),
            fails("no-cwd", "invalid_arguments", "context_status", "{}"),
        ],
    ),
    case("skills_list", &[read("", "skills_list", "{}")]),
    case(
        "skills_get",
        &[
            read("found", "skills_get", r#"{"name":"demo"}"#),
            fails("missing", "not_found", "skills_get", r#"{"name":"nope"}"#),
        ],
    ),
    case(
        "skills_lint",
        &[
            read("", "skills_lint", "{}"),
            hook("files", World::checks_files),
            read("checks", "skills_lint", "{}"),
        ],
    ),
    prepared(
        "sources_ls",
        sources_home,
        &[
            read("summary", "sources_ls", "{}"),
            read("items", "sources_ls", r#"{"items":true,"kind":"skill"}"#),
            read("org", "sources_ls", r#"{"source":"org","items":true}"#),
            fails(
                "bad_kind",
                "invalid_arguments",
                "sources_ls",
                r#"{"kind":"nope"}"#,
            ),
        ],
    ),
    case(
        "skills_files",
        &[
            read("demo", "skills_files", r#"{"name":"demo"}"#),
            hook("files", World::checks_files),
            read("acceptance", "skills_files", r#"{"name":"acceptance"}"#),
            setup("skills_sync", CLAUDE_APPLY),
            read(
                "synced",
                "skills_files",
                r#"{"name":"acceptance","client_keys":["claude-code"]}"#,
            ),
            fails("missing", "not_found", "skills_files", r#"{"name":"nope"}"#),
        ],
    ),
    case(
        "skills_fix",
        &[
            hook("files", World::checks_files),
            read("preview", "skills_fix", r#"{"fix":"chmod"}"#),
            refused(
                "refused",
                "skills_fix",
                r#"{"fix":"chmod","dry_run":false}"#,
            ),
            write(
                "apply",
                "skills_fix",
                r#"{"fix":"chmod","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "unknown",
                "invalid_arguments",
                "skills_fix",
                r#"{"fix":"delete"}"#,
            ),
            fails(
                "missing_skill",
                "not_found",
                "skills_fix",
                r#"{"fix":"chmod","names":["nope"]}"#,
            ),
            hook("outside-link", outside_link),
            read(
                "move_link_preview",
                "skills_fix",
                r#"{"fix":"move-link","names":["acceptance"],"link":"../../shared/rubric.md"}"#,
            ),
            refused(
                "move_link_refused",
                "skills_fix",
                r#"{"fix":"move-link","names":["acceptance"],"link":"../../shared/rubric.md","dry_run":false}"#,
            ),
            write(
                "move_link",
                "skills_fix",
                r#"{"fix":"move-link","names":["acceptance"],"link":"../../shared/rubric.md","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "move_link_not_outside",
                "refused",
                "skills_fix",
                r#"{"fix":"move-link","names":["acceptance"],"link":"references/guide.md"}"#,
            ),
            fails(
                "move_link_no_link",
                "invalid_arguments",
                "skills_fix",
                r#"{"fix":"move-link","names":["acceptance"]}"#,
            ),
        ],
    ),
    case(
        "skills_status",
        &[
            read("fresh", "skills_status", "{}"),
            setup("skills_sync", CLAUDE_APPLY),
            read("synced", "skills_status", CLAUDE),
            fails(
                "unknown_client",
                "invalid_arguments",
                "skills_status",
                r#"{"client_keys":["cursor"]}"#,
            ),
        ],
    ),
    case(
        "skills_list_transpilers",
        &[read("", "skills_list_transpilers", "{}")],
    ),
    case(
        "skills_scaffold",
        &[
            write("skill", "skills_scaffold", r#"{"name":"fresh"}"#),
            write(
                "rule",
                "skills_scaffold",
                r#"{"name":"house-rule","skill_type":"rule"}"#,
            ),
            fails(
                "exists",
                "conflict",
                "skills_scaffold",
                r#"{"name":"fresh"}"#,
            ),
            fails(
                "escaping_name",
                "invalid_arguments",
                "skills_scaffold",
                r#"{"name":"../escape"}"#,
            ),
            fails(
                "unknown_type",
                "invalid_arguments",
                "skills_scaffold",
                r#"{"name":"odd","skill_type":"bogus"}"#,
            ),
        ],
    ),
    case(
        "skills_sync",
        &[
            read("preview", "skills_sync", CLAUDE),
            refused("refused", "skills_sync", CLAUDE_UNCONFIRMED),
            write("apply", "skills_sync", CLAUDE_APPLY),
            read("unattended", "skills_sync", CLAUDE_UNATTENDED),
            setup("skills_auto_pause", "{}"),
            read("unattended_paused", "skills_sync", CLAUDE_UNATTENDED),
            fails(
                "unattended_apply_held",
                "invalid_arguments",
                "skills_sync",
                r#"{"unattended":true,"apply_held":true}"#,
            ),
            fails(
                "unattended_take",
                "invalid_arguments",
                "skills_sync",
                r#"{"unattended":true,"take":["acceptance/SETUP.md"]}"#,
            ),
            hook("files", World::checks_files),
            hook("differs", differing_setup),
            read(
                "take_preview",
                "skills_sync",
                r#"{"client_keys":["claude-code"],"only":["acceptance"],"take":["acceptance/SETUP.md"]}"#,
            ),
            write(
                "take",
                "skills_sync",
                r#"{"client_keys":["claude-code"],"only":["acceptance"],"take":["acceptance/SETUP.md"],"dry_run":false,"confirm":true}"#,
            ),
            fails(
                "unknown_client",
                "invalid_arguments",
                "skills_sync",
                r#"{"client_keys":["cursor"],"dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "skills_undo",
        &[
            fails("none", "not_found", "skills_undo", "{}"),
            setup("skills_sync", CLAUDE_APPLY),
            read("preview", "skills_undo", "{}"),
            refused("refused", "skills_undo", UNCONFIRMED),
            write("apply", "skills_undo", APPLY),
            fails("again", "not_found", "skills_undo", APPLY),
        ],
    ),
    case(
        "skills_verify",
        &[
            read("none", "skills_verify", "{}"),
            setup("skills_sync", CLAUDE_APPLY),
            read("synced", "skills_verify", "{}"),
            fails(
                "unknown_client",
                "invalid_arguments",
                "skills_verify",
                r#"{"client_keys":["cursor"]}"#,
            ),
        ],
    ),
    case(
        "skills_auto_status",
        &[
            read("off", "skills_auto_status", "{}"),
            setup("skills_auto_pause", "{}"),
            read("paused", "skills_auto_status", "{}"),
        ],
    ),
    case(
        "skills_auto_on",
        &[
            read("preview", "skills_auto_on", r#"{"dry_run":true}"#),
            Call {
                writes: true,
                ..fails("no_broker", "approval_unavailable", "skills_auto_on", "{}")
            },
        ],
    ),
    case(
        "skills_auto_off",
        &[
            read("already_off", "skills_auto_off", r#"{"dry_run":true}"#),
            write("off", "skills_auto_off", "{}"),
        ],
    ),
    case(
        "skills_auto_pause",
        &[
            write("pause", "skills_auto_pause", "{}"),
            write("again", "skills_auto_pause", "{}"),
        ],
    ),
    case(
        "skills_auto_resume",
        &[
            write("not_paused", "skills_auto_resume", "{}"),
            setup("skills_auto_pause", "{}"),
            write("resume", "skills_auto_resume", "{}"),
        ],
    ),
    case(
        "skills_diff",
        &[
            read("fresh", "skills_diff", "{}"),
            setup("skills_sync", CLAUDE_APPLY),
            read("synced", "skills_diff", "{}"),
            hook("files", World::checks_files),
            hook("differs", differing_setup),
            read("held", "skills_diff", r#"{"name":"acceptance"}"#),
            read(
                "file",
                "skills_diff",
                r#"{"name":"acceptance","file":"SETUP.md"}"#,
            ),
            fails(
                "unknown_file",
                "not_found",
                "skills_diff",
                r#"{"name":"acceptance","file":"nope.md"}"#,
            ),
            fails(
                "file_alone",
                "invalid_arguments",
                "skills_diff",
                r#"{"file":"SETUP.md"}"#,
            ),
        ],
    ),
    case(
        "skills_keep",
        &[
            hook("files", World::checks_files),
            hook("differs", differing_setup),
            read(
                "preview",
                "skills_keep",
                r#"{"name":"acceptance","files":["SETUP.md"]}"#,
            ),
            refused(
                "refused",
                "skills_keep",
                r#"{"name":"acceptance","files":["SETUP.md"],"dry_run":false}"#,
            ),
            write(
                "apply",
                "skills_keep",
                r#"{"name":"acceptance","files":["SETUP.md"],"dry_run":false,"confirm":true}"#,
            ),
            fails(
                "not_held",
                "refused",
                "skills_keep",
                r#"{"name":"acceptance","files":["references/guide.md"]}"#,
            ),
            fails(
                "no_files",
                "invalid_arguments",
                "skills_keep",
                r#"{"name":"acceptance"}"#,
            ),
        ],
    ),
    case("skills_audit", &[read("", "skills_audit", "{}")]),
    case("skills_tap_list", &[read("empty", "skills_tap_list", "{}")]),
    case(
        "skills_search",
        &[read("empty", "skills_search", r#"{"query":"tapskill"}"#)],
    ),
    case(
        "skills_tap_add",
        &[
            read("preview", "skills_tap_add", r#"{"repo":"acme/skills"}"#),
            fails(
                "escaping_name",
                "invalid_arguments",
                "skills_tap_add",
                r#"{"repo":"acme/skills","name":"../x"}"#,
            ),
        ],
    ),
    case(
        "skills_tap_remove",
        &[
            fails(
                "unknown",
                "not_found",
                "skills_tap_remove",
                r#"{"name":"local"}"#,
            ),
            setup("skills_tap_add", ADD_TAP),
            read("preview", "skills_tap_remove", r#"{"name":"local"}"#),
            write(
                "apply",
                "skills_tap_remove",
                r#"{"name":"local","dry_run":false}"#,
            ),
        ],
    ),
    case(
        "skills_tap_update",
        &[read("none", "skills_tap_update", "{}")],
    ),
    prepared(
        "library_select_ls",
        taps_world::select_src,
        &[
            read("empty", "library_select_ls", "{}"),
            fails(
                "unknown",
                "not_found",
                "library_select_ls",
                r#"{"source":"nope"}"#,
            ),
            setup(
                "skills_tap_add",
                r#"{"repo":"{base}/select-src","name":"picks","dry_run":false}"#,
            ),
            read("items", "library_select_ls", r#"{"source":"picks"}"#),
        ],
    ),
    prepared(
        "library_select_plan",
        taps_world::select_src,
        &[
            fails("missing", "invalid_arguments", "library_select_plan", "{}"),
            setup(
                "skills_tap_add",
                r#"{"repo":"{base}/select-src","name":"picks","dry_run":false}"#,
            ),
            read("plan", "library_select_plan", r#"{"source":"picks"}"#),
            read(
                "deepen",
                "library_select_plan",
                r#"{"source":"picks","deepen":true}"#,
            ),
            read(
                "local",
                "library_select_plan",
                r#"{"source":"picks","fetch":false}"#,
            ),
        ],
    ),
    prepared(
        "library_select_set",
        taps_world::select_src,
        &[
            setup("skills_tap_add", SELECT_TAP),
            fails(
                "missing",
                "invalid_arguments",
                "library_select_set",
                r#"{"source":"picks"}"#,
            ),
            read(
                "preview",
                "library_select_set",
                r#"{"source":"picks","on":["code-review","agent:reviewer"]}"#,
            ),
            refused(
                "refused",
                "library_select_set",
                r#"{"source":"picks","on":["code-review"],"dry_run":false}"#,
            ),
            fails(
                "hook_stays_with_a_person",
                "invalid_arguments",
                "library_select_set",
                r#"{"source":"picks","on":["PostToolUse:Write|Edit"],"dry_run":false,"confirm":true}"#,
            ),
            write(
                "apply",
                "library_select_set",
                r#"{"source":"picks","on":["code-review","agent:reviewer"],"dry_run":false,"confirm":true}"#,
            ),
            write(
                "off",
                "library_select_set",
                r#"{"source":"picks","off":["agent:reviewer"],"dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    prepared(
        "library_select_apply",
        select_with_library,
        &[
            setup("skills_tap_add", SELECT_TAP),
            setup("library_select_set", SELECT_CHOOSE),
            read("plan", "library_select_apply", r#"{"source":"picks"}"#),
            fails(
                "no_hash",
                "invalid_arguments",
                "library_select_apply",
                r#"{"source":"picks","dry_run":false,"confirm":true}"#,
            ),
            refused(
                "refused",
                "library_select_apply",
                r#"{"source":"picks","plan_hash":"sha256:0000000000000000","dry_run":false}"#,
            ),
            fails(
                "wrong_hash",
                "conflict",
                "library_select_apply",
                r#"{"source":"picks","plan_hash":"sha256:0000000000000000","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "no_force",
                "invalid_arguments",
                "library_select_apply",
                r#"{"source":"picks","plan_hash":"sha256:0000000000000000","force":["style"],"dry_run":false,"confirm":true}"#,
            ),
            write(
                "apply",
                "library_select_apply",
                r#"{"source":"picks","plan_hash":"sha256:4fa22760680f65ff","dry_run":false,"confirm":true}"#,
            ),
            Call {
                label: "choose_rule",
                ..setup(
                    "library_select_set",
                    r#"{"source":"picks","on":["style"],"dry_run":false,"confirm":true}"#,
                )
            },
            read(
                "rules_plan",
                "library_select_apply",
                r#"{"source":"picks"}"#,
            ),
            fails(
                "rules_unknown_skip",
                "conflict",
                "library_select_apply",
                r#"{"source":"picks","plan_hash":"sha256:d4b8e6eb5001ce78","skip":["nope"],"dry_run":false,"confirm":true}"#,
            ),
            write(
                "rules_apply",
                "library_select_apply",
                r#"{"source":"picks","plan_hash":"sha256:d4b8e6eb5001ce78","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    prepared(
        "library_select_undo",
        taps_world::select_src,
        &[
            setup("skills_tap_add", SELECT_TAP),
            setup("library_select_set", SELECT_CHOOSE),
            refused("refused", "library_select_undo", r#"{"source":"picks"}"#),
            fails(
                "nothing",
                "not_found",
                "library_select_undo",
                r#"{"source":"picks","confirm":true}"#,
            ),
            setup(
                "library_select_apply",
                r#"{"source":"picks","plan_hash":"sha256:4fa22760680f65ff","dry_run":false,"confirm":true}"#,
            ),
            write(
                "undo",
                "library_select_undo",
                r#"{"source":"picks","confirm":true}"#,
            ),
        ],
    ),
    prepared(
        "library_select_check",
        taps_world::select_src,
        &[
            write("none", "library_select_check", "{}"),
            setup("skills_tap_add", SELECT_TAP),
            setup("library_select_set", SELECT_CHOOSE),
            write("waiting", "library_select_check", r#"{"source":"picks"}"#),
            write(
                "local",
                "library_select_check",
                r#"{"fetch":false,"deepen":true}"#,
            ),
            fails(
                "unknown",
                "not_found",
                "library_select_check",
                r#"{"source":"nope"}"#,
            ),
        ],
    ),
    prepared(
        "skills_tap_link",
        taps_world::both,
        &[
            fails(
                "unknown",
                "not_found",
                "skills_tap_link",
                r#"{"name":"plugins"}"#,
            ),
            setup(
                "skills_tap_add",
                r#"{"repo":"{base}/plugin-src","name":"plugins","dry_run":false}"#,
            ),
            read("preview", "skills_tap_link", r#"{"name":"plugins"}"#),
            fails(
                "unknown_plugin",
                "invalid_arguments",
                "skills_tap_link",
                r#"{"name":"plugins","plugins":["nope"]}"#,
            ),
            write(
                "apply",
                "skills_tap_link",
                r#"{"name":"plugins","dry_run":false}"#,
            ),
        ],
    ),
    prepared(
        "skills_tap_unlink",
        taps_world::both,
        &[
            setup(
                "skills_tap_add",
                r#"{"repo":"{base}/plugin-src","name":"plugins","dry_run":false}"#,
            ),
            read("not_linked", "skills_tap_unlink", r#"{"name":"plugins"}"#),
            setup("skills_tap_link", r#"{"name":"plugins","dry_run":false}"#),
            read("preview", "skills_tap_unlink", r#"{"name":"plugins"}"#),
            write(
                "apply",
                "skills_tap_unlink",
                r#"{"name":"plugins","dry_run":false}"#,
            ),
        ],
    ),
    case(
        "skills_install",
        &[
            read("preview", "skills_install", r#"{"spec":"@acme/skills"}"#),
            fails(
                "malformed_spec",
                "invalid_arguments",
                "skills_install",
                r#"{"spec":"no-owner-or-repo"}"#,
            ),
        ],
    ),
    setup_case(
        "setup_parts",
        &[read("roots", "setup_parts", r#"{"roots":["{home}/code"]}"#)],
    ),
    setup_case(
        "setup_export",
        &[
            read(
                "preview",
                "setup_export",
                r#"{"out":"{base}/out","roots":["{home}/code"]}"#,
            ),
            refused(
                "refused",
                "setup_export",
                r#"{"out":"{base}/out/a.zip","roots":["{home}/code"],"dry_run":false}"#,
            ),
            fails(
                "sealed_parts",
                "refused",
                "setup_export",
                r#"{"parts":"all,secrets","out":"{base}/out","dry_run":true}"#,
            ),
            write(
                "apply",
                "setup_export",
                r#"{"out":"{base}/out/a.zip","roots":["{home}/code"],"dry_run":false,"confirm":true}"#,
            ),
            fails(
                "exists",
                "conflict",
                "setup_export",
                r#"{"out":"{base}/out/a.zip","roots":["{home}/code"],"dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    setup_case(
        "setup_check",
        &[read("roots", "setup_check", r#"{"roots":["{home}/code"]}"#)],
    ),
    setup_case(
        "setup_inspect",
        &[
            setup("setup_export", SETUP_EXPORT_APPLY),
            read("file", "setup_inspect", r#"{"file":"{base}/out/a.zip"}"#),
        ],
    ),
    setup_case(
        "setup_restore",
        &[
            setup("setup_export", SETUP_EXPORT_APPLY),
            read("plan", "setup_restore", r#"{"file":"{base}/out/a.zip"}"#),
            refused(
                "refused",
                "setup_restore",
                r#"{"file":"{base}/out/a.zip","dry_run":false}"#,
            ),
        ],
    ),
    case("sync_status", &[read("unconfigured", "sync_status", "{}")]),
    case("sync_plan", &[read("unconfigured", "sync_plan", "{}")]),
    prepared(
        "sync_repos",
        server_repo_with_peers,
        &[read("compared", "sync_repos", "{}")],
    ),
    case(
        "skills_bundle",
        &[
            read("preview", "skills_bundle", "{}"),
            write("apply", "skills_bundle", r#"{"dry_run":false}"#),
            fails(
                "exists",
                "conflict",
                "skills_bundle",
                r#"{"dry_run":false}"#,
            ),
        ],
    ),
    case(
        "skills_unbundle",
        &[
            setup("skills_bundle", r#"{"dry_run":false}"#),
            setup(
                "skills_uninstall",
                r#"{"name":"demo","dry_run":false,"confirm":true}"#,
            ),
            read("preview", "skills_unbundle", BUNDLE),
            refused(
                "refused",
                "skills_unbundle",
                r#"{"bundle_path":"{repo}/skills-repo-bundle.zip","dry_run":false}"#,
            ),
            write(
                "apply",
                "skills_unbundle",
                r#"{"bundle_path":"{repo}/skills-repo-bundle.zip","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "missing_bundle",
                "not_found",
                "skills_unbundle",
                r#"{"bundle_path":"{repo}/none.zip","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "skills_clean",
        &[
            setup("skills_sync", CLAUDE_APPLY),
            read("preview", "skills_clean", "{}"),
            refused("refused", "skills_clean", UNCONFIRMED),
            fails(
                "unknown_client",
                "invalid_arguments",
                "skills_clean",
                r#"{"client":"cursor"}"#,
            ),
            write("apply", "skills_clean", APPLY),
        ],
    ),
    case(
        "skills_uninstall",
        &[
            setup("skills_sync", CLAUDE_APPLY),
            read("preview", "skills_uninstall", r#"{"name":"demo"}"#),
            refused(
                "refused",
                "skills_uninstall",
                r#"{"name":"demo","dry_run":false}"#,
            ),
            fails(
                "escaping_name",
                "invalid_arguments",
                "skills_uninstall",
                r#"{"name":"../agents/helper","dry_run":false,"confirm":true}"#,
            ),
            write(
                "apply",
                "skills_uninstall",
                r#"{"name":"demo","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "gone",
                "not_found",
                "skills_uninstall",
                r#"{"name":"demo","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "skills_resolve",
        &[
            setup("skills_sync", CLAUDE_APPLY),
            read("preview", "skills_resolve", "{}"),
            refused("refused", "skills_resolve", UNCONFIRMED),
            fails(
                "unknown_client",
                "invalid_arguments",
                "skills_resolve",
                r#"{"client":"cursor"}"#,
            ),
            write("apply", "skills_resolve", APPLY),
        ],
    ),
    // compression
    case(
        "spend_status",
        &[
            read("off", "spend_status", "{}"),
            hook("data", spend_data),
            read("with_data", "spend_status", "{}"),
        ],
    ),
    case(
        "spend_summary",
        &[
            read("empty", "spend_summary", "{}"),
            hook("data", spend_data),
            read(
                "model_and_task",
                "spend_summary",
                r#"{"range":"30d","model":"claude-sonnet-4","task":"Interactive"}"#,
            ),
            fails(
                "bad_range",
                "invalid_arguments",
                "spend_summary",
                r#"{"range":"year"}"#,
            ),
        ],
    ),
    case(
        "spend_setup_plan",
        &[
            hook("settings", spend_settings),
            read("plan", "spend_setup_plan", "{}"),
            read("port", "spend_setup_plan", r#"{"port":"4999"}"#),
        ],
    ),
    case(
        "spend_receiver_status",
        &[
            read("off", "spend_receiver_status", "{}"),
            hook("data", spend_data),
            read("with_data", "spend_receiver_status", "{}"),
        ],
    ),
    case(
        "spend_export_csv",
        &[
            read("empty", "spend_export_csv", "{}"),
            hook("data", spend_data),
            read("month", "spend_export_csv", r#"{"range":"30d"}"#),
            fails(
                "bad_range",
                "invalid_arguments",
                "spend_export_csv",
                r#"{"range":"year"}"#,
            ),
        ],
    ),
    case(
        "spend_config_set",
        &[
            read(
                "preview",
                "spend_config_set",
                r#"{"retention_days":30,"show_cost":false}"#,
            ),
            refused(
                "refused",
                "spend_config_set",
                r#"{"retention_days":30,"dry_run":false}"#,
            ),
            fails("nothing", "invalid_arguments", "spend_config_set", "{}"),
            fails(
                "invalid",
                "invalid_arguments",
                "spend_config_set",
                r#"{"port":"99999999"}"#,
            ),
            write(
                "apply",
                "spend_config_set",
                r#"{"retention_days":30,"max_mb":64,"show_cost":false,"dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "spend_setup_apply",
        &[
            hook("settings", spend_settings),
            read("preview", "spend_setup_apply", r#"{"port":"4999"}"#),
            refused(
                "refused",
                "spend_setup_apply",
                r#"{"port":"4999","dry_run":false}"#,
            ),
            write(
                "conflict",
                "spend_setup_apply",
                r#"{"port":"4999","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "no_overwrite",
                "invalid_arguments",
                "spend_setup_apply",
                r#"{"overwrite":["all"],"dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "spend_setup_undo",
        &[
            hook("settings", spend_settings),
            read("nothing", "spend_setup_undo", "{}"),
            setup(
                "spend_setup_apply",
                r#"{"port":"4999","dry_run":false,"confirm":true}"#,
            ),
            read("preview", "spend_setup_undo", "{}"),
            refused("refused", "spend_setup_undo", r#"{"dry_run":false}"#),
            write(
                "apply",
                "spend_setup_undo",
                r#"{"dry_run":false,"confirm":true}"#,
            ),
            fails(
                "no_delete_data",
                "invalid_arguments",
                "spend_setup_undo",
                r#"{"delete_data":true,"dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "compression_status",
        &[
            read("fresh", "compression_status", "{}"),
            setup("compression_enable", ENABLE_OFF_PORT),
            read("enabled", "compression_status", "{}"),
        ],
    ),
    case(
        "compression_measure",
        &[
            read("empty", "compression_measure", "{}"),
            hook("world", compression::measure_world),
            read("measured", "compression_measure", "{}"),
            read(
                "since",
                "compression_measure",
                r#"{"since":"2026-11-10","min_turns":3}"#,
            ),
            fails(
                "bad_since",
                "invalid_arguments",
                "compression_measure",
                r#"{"since":"last week"}"#,
            ),
            fails(
                "min_turns_not_a_number",
                "invalid_arguments",
                "compression_measure",
                r#"{"min_turns":"five"}"#,
            ),
            read("csv", "compression_measure", r#"{"format":"csv"}"#),
            fails(
                "bad_format",
                "invalid_arguments",
                "compression_measure",
                r#"{"format":"xml"}"#,
            ),
        ],
    ),
    case(
        "compression_hooks_pause",
        &[
            write("for", "compression_hooks_pause", r#"{"for":"1d"}"#),
            write("open", "compression_hooks_pause", "{}"),
            fails(
                "bad_for",
                "invalid_arguments",
                "compression_hooks_pause",
                r#"{"for":"soon"}"#,
            ),
            fails(
                "both",
                "invalid_arguments",
                "compression_hooks_pause",
                r#"{"for":"1d","until":"2099-01-01"}"#,
            ),
            fails(
                "too_long",
                "invalid_arguments",
                "compression_hooks_pause",
                r#"{"for":"31d"}"#,
            ),
        ],
    ),
    case(
        "compression_hooks_resume",
        &[
            setup("compression_hooks_pause", r#"{"for":"1d"}"#),
            write("apply", "compression_hooks_resume", "{}"),
            read("again", "compression_hooks_resume", "{}"),
        ],
    ),
    case(
        "compression_hooks_adopt_edit",
        &[
            hook("home", compression::edited_rtk_home),
            write("apply", "compression_hooks_adopt_edit", r#"{"hook":"rtk"}"#),
            write(
                "revert",
                "compression_hooks_adopt_edit",
                r#"{"hook":"rtk","revert":true}"#,
            ),
            fails(
                "not_edited",
                "backend_error",
                "compression_hooks_adopt_edit",
                r#"{"hook":"read-once"}"#,
            ),
            fails(
                "unknown_hook",
                "invalid_arguments",
                "compression_hooks_adopt_edit",
                r#"{"hook":"nope"}"#,
            ),
            fails(
                "hook_missing",
                "invalid_arguments",
                "compression_hooks_adopt_edit",
                "{}",
            ),
        ],
    ),
    case(
        "compression_hooks_verify",
        &[
            hook("home", compression::fixture_hooks),
            read("both", "compression_hooks_verify", "{}"),
            read("one", "compression_hooks_verify", r#"{"hook":"read-once"}"#),
            fails(
                "unknown_hook",
                "invalid_arguments",
                "compression_hooks_verify",
                r#"{"hook":"nope"}"#,
            ),
            hook("silent", compression::silent_hooks),
            read("failing", "compression_hooks_verify", "{}"),
        ],
    ),
    case(
        "compression_enable",
        &[
            read(
                "preview",
                "compression_enable",
                r#"{"provider":"rtk-only","port":29213,"mode":"cache"}"#,
            ),
            refused(
                "refused",
                "compression_enable",
                r#"{"provider":"rtk-only","port":29213,"mode":"cache","dry_run":false}"#,
            ),
            fails(
                "unknown_provider",
                "invalid_arguments",
                "compression_enable",
                r#"{"provider":"bogus","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "port_not_a_number",
                "invalid_arguments",
                "compression_enable",
                r#"{"port":"29213","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "parsec",
                "invalid_arguments",
                "compression_enable",
                r#"{"provider":"parsec","dry_run":false,"confirm":true}"#,
            ),
            write(
                "apply",
                "compression_enable",
                r#"{"provider":"rtk-only","port":29213,"mode":"cache","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "compression_disable",
        &[
            setup("compression_enable", ENABLE_OFF_PORT),
            read("preview", "compression_disable", "{}"),
            read("teardown_preview", "compression_disable", TEARDOWN),
            refused("refused", "compression_disable", UNCONFIRMED),
            refused(
                "teardown_refused",
                "compression_disable",
                TEARDOWN_UNCONFIRMED,
            ),
            write("apply", "compression_disable", APPLY),
        ],
    ),
    case(
        "compression_set_provider",
        &[
            read(
                "preview",
                "compression_set_provider",
                r#"{"provider":"headroom"}"#,
            ),
            refused(
                "refused",
                "compression_set_provider",
                r#"{"provider":"headroom","dry_run":false}"#,
            ),
            write(
                "apply",
                "compression_set_provider",
                r#"{"provider":"headroom","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "compression_use",
        &[
            read("preview", "compression_use", r#"{"preset":"agent"}"#),
            refused(
                "refused",
                "compression_use",
                r#"{"preset":"agent","dry_run":false}"#,
            ),
            write(
                "apply",
                "compression_use",
                r#"{"preset":"agent","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    prepared(
        "compression_sync",
        demo_skill_file,
        &[
            read("preview", "compression_sync", "{}"),
            read(
                "root_preview",
                "compression_sync",
                r#"{"mcpm_root":"{home}"}"#,
            ),
            fails(
                "root_relative",
                "invalid_arguments",
                "compression_sync",
                r#"{"mcpm_root":"relative/mcpm"}"#,
            ),
            fails(
                "root_missing",
                "invalid_arguments",
                "compression_sync",
                r#"{"mcpm_root":"{home}/absent"}"#,
            ),
            fails(
                "root_file",
                "invalid_arguments",
                "compression_sync",
                r#"{"mcpm_root":"{base}/skills-repo/skills/demo/SKILL.md"}"#,
            ),
            refused("refused", "compression_sync", UNCONFIRMED),
            write("apply", "compression_sync", APPLY),
        ],
    ),
    // seal reads the proxy's /health, even for a preview: first a port nothing listens on, then
    // one that answers like the proxy
    prepared(
        "compression_seal",
        compression::fake_proxy,
        &[
            setup("compression_enable", ENABLE_OFF_PORT),
            refused("refused", "compression_seal", UNCONFIRMED),
            fails("no_proxy", "backend_error", "compression_seal", APPLY),
            Call {
                label: "enable-proxy",
                ..setup("compression_enable", ENABLE_PROXY_PORT)
            },
            read("preview", "compression_seal", "{}"),
            write("apply", "compression_seal", APPLY),
            read("sealed", "compression_seal", APPLY),
        ],
    ),
];

fn stem(case: &Case, call: &Call) -> String {
    if call.label.is_empty() {
        case.tool.to_string()
    } else {
        format!("{}.{}", case.tool, call.label)
    }
}

fn resource_stem(uri: &str) -> String {
    format!(
        "resource-{}",
        uri.trim_start_matches("mcpm://").replace('/', "-")
    )
}

fn changed_files(
    world: &World,
    before: &std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
) -> Vec<String> {
    let after = world.snapshot();
    after
        .iter()
        .filter(|(path, _)| !path.ends_with(".git/FETCH_HEAD"))
        .filter(|(path, bytes)| before.get(*path) != Some(*bytes))
        .map(|(path, _)| path.display().to_string())
        .chain(
            before
                .keys()
                .filter(|p| !after.contains_key(*p))
                .map(|p| format!("removed {}", p.display())),
        )
        .collect()
}

fn arguments(world: &World, case: &Case, call: &Call) -> Value {
    let expanded = call
        .args
        .replace("{home}", &world.path(&world.home))
        .replace("{repo}", &world.repo.to_string_lossy())
        .replace("{data}", &world.data.to_string_lossy())
        .replace("{base}", &world.base.to_string_lossy());
    serde_json::from_str(&expanded)
        .unwrap_or_else(|e| panic!("{} {}: arguments are not JSON: {e}", case.tool, call.label))
}

fn select_with_library(world: &World) {
    taps_world::select_src(world);
    taps_world::library_with_style(world);
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

fn claude_code_home(world: &World) {
    std::fs::write(
        world.home.join(".claude.json"),
        r#"{"theme": "dark", "mcpServers": {"legacy-lint": {"command": "lint", "args": []}}}"#,
    )
    .unwrap();
}

fn launch_profile_world(world: &World) {
    world.context_files();
    claude_code_home(world);
    let mut registry = world.registry();
    registry["version"] = serde_json::json!(3);
    registry["servers"][0]["enabled"] = serde_json::json!(true);
    registry["servers"][1]["enabled"] = serde_json::json!(true);
    registry["defaultAccessProfileId"] = serde_json::json!("default");
    registry["profiles"] = serde_json::json!([
        {"id": "default", "name": "Default", "enabledServerIds": ["srv-alpha", "srv-beta"]},
        {"id": "bare", "name": "Bare", "enabledServerIds": ["srv-alpha"]},
        {"id": "empty", "name": "Empty", "enabledServerIds": []},
    ]);
    world.write_registry(&registry);
}

fn folder_routing_world(world: &World) {
    let mut registry = world.registry();
    registry["version"] = serde_json::json!(3);
    registry["servers"][0]["enabled"] = serde_json::json!(true);
    registry["servers"][1]["enabled"] = serde_json::json!(true);
    registry["defaultAccessProfileId"] = serde_json::json!("default");
    registry["profiles"] = serde_json::json!([
        {"id": "default", "name": "Default", "enabledServerIds": ["srv-alpha"]},
        {"id": "focus", "name": "Focus", "enabledServerIds": ["srv-alpha"]},
        {"id": "wide", "name": "Wide", "enabledServerIds": ["srv-alpha", "srv-beta"]},
    ]);
    registry["clientScopes"] = serde_json::json!({"claude-code": "focus"});
    registry["folderProfiles"] = serde_json::json!([{"path": "/work/proj", "profile": "wide"}]);
    world.write_registry(&registry);
}

fn spend_data(world: &World) {
    common::spend_world::build(world);
}

fn spend_settings(world: &World) {
    common::spend_world::settings_with_a_conflict(world);
}

fn sources_home(world: &World) {
    common::sources_world::build_in(&world.base);
}

fn demo_skill_file(world: &World) {
    let file = world.base.join("skills-repo/skills/demo/SKILL.md");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, "# Demo\n").unwrap();
}

/// A local git repository with one commit, so a tap can be added without a network.
fn tap_source(world: &World) {
    let dir = world.base.join("tap-src");
    let skill = dir.join("skills/tapskill/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        skill,
        "---\nname: tapskill\ndescription: A synthetic tap skill\n---\nTap body\n",
    )
    .unwrap();
    for args in [
        &["init", "-q"][..],
        &["symbolic-ref", "HEAD", "refs/heads/main"],
        &["add", "-A"],
        &["commit", "-q", "-m", "fixture"],
    ] {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &world.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    }
}

fn run_case(case: &Case) {
    let world = World::new(case.tool);
    if case.tool == "skills_tap_remove" {
        tap_source(&world);
    }
    if let Some(prepare) = case.prepare {
        prepare(&world);
    }
    let mut client = Client::spawn(&world, &[FAKE_SECRET, SECRET_KEY]);
    client.handshake();
    for call in case.calls {
        if let Some(prepare) = call.hook {
            prepare(&world);
            continue;
        }
        let name = format!("{} {}", case.tool, call.label);
        let args = arguments(&world, case, call);
        let before = world.snapshot();
        let reply = client.call(call.tool, args.clone());
        match call.expect {
            Expect::Ok => {
                assert!(
                    !reply.is_error(),
                    "{name}: expected success, got {}",
                    reply.0
                );
                let parsed: Value = serde_json::from_str(reply.text())
                    .unwrap_or_else(|e| panic!("{name}: text is not the JSON payload: {e}"));
                assert_eq!(
                    &parsed,
                    reply.data(),
                    "{name}: text and structuredContent must agree"
                );
            }
            Expect::Error(kind) => {
                assert!(
                    reply.is_error(),
                    "{name}: expected an error, got {}",
                    reply.0
                );
                assert_eq!(reply.error_kind(), Some(kind), "{name}: {}", reply.0);
                assert!(
                    reply.text().starts_with(&format!("{kind}: ")),
                    "{name}: text {:?}",
                    reply.text()
                );
            }
        }
        if !call.writes {
            let changed = changed_files(&world, &before);
            assert!(
                changed.is_empty(),
                "{name}: a read, a refusal or a failure changed the world: {changed:?}"
            );
        }
        let stem = stem(case, call);
        if call.golden {
            let mut result = json!({
                "tool": call.tool,
                "arguments": args,
                "isError": reply.is_error(),
                "result": reply.data(),
            });
            if reply.is_error() {
                result["text"] = json!(reply.text());
            }
            golden::assert_golden_in(
                &golden::selfmcp_root(),
                &stem,
                &normalize::normalize(&world, &result),
            );
        }
    }
    assert!(client.close().success(), "{}: server exit", case.tool);
}

#[test]
fn every_tool_call_matches_its_golden_result() {
    let failures: Vec<String> = CASES
        .iter()
        .filter_map(|case| {
            std::panic::catch_unwind(|| run_case(case))
                .err()
                .map(|payload| {
                    let message = payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default();
                    format!("{}: {message}", case.tool)
                })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn every_resource_matches_its_golden_result() {
    let world = World::new("resources");
    let mut client = Client::spawn(&world, &[FAKE_SECRET, SECRET_KEY]);
    client.handshake();
    let before = world.snapshot();
    let mut seen = BTreeSet::new();
    for def in RESOURCES {
        let content = client.read(def.uri);
        assert_eq!(content["uri"], def.uri);
        assert_eq!(content["mimeType"], def.mime);
        let text = content["text"].as_str().expect("text content");
        let body = if def.mime == "application/json" {
            json!({"uri": def.uri, "mimeType": def.mime,
                   "json": serde_json::from_str::<Value>(text)
                       .unwrap_or_else(|e| panic!("{} is not JSON ({e}): {text}", def.uri))})
        } else {
            json!({"uri": def.uri, "mimeType": def.mime, "text": text})
        };
        let stem = resource_stem(def.uri);
        assert!(seen.insert(stem.clone()), "{} collides", def.uri);
        golden::assert_golden_in(
            &golden::selfmcp_root(),
            &stem,
            &normalize::normalize(&world, &body),
        );
    }
    assert_eq!(changed_files(&world, &before), Vec::<String>::new());
    assert!(client.close().success());
}

fn tool_names() -> BTreeSet<&'static str> {
    TOOLS.iter().map(|t| t.name).collect()
}

#[test]
fn every_case_names_a_tool_and_every_golden_belongs_to_a_case() {
    let names = tool_names();
    let mut owned: BTreeSet<String> = RESOURCES.iter().map(|r| resource_stem(r.uri)).collect();
    let mut seen = BTreeSet::new();
    for case in CASES {
        assert!(
            names.contains(case.tool),
            "case `{}` is not a catalog tool",
            case.tool
        );
        assert!(seen.insert(case.tool), "case `{}` appears twice", case.tool);
        let labels: BTreeSet<&str> = case.calls.iter().map(|c| c.label).collect();
        assert_eq!(
            labels.len(),
            case.calls.len(),
            "{}: labels repeat",
            case.tool
        );
        for call in case.calls {
            // A setup call (no golden) may prepare the world with another tool.
            assert!(
                call.tool == case.tool || !call.golden,
                "{}: a case calls its own tool",
                call.label
            );
            if call.golden {
                owned.insert(stem(case, call));
            }
        }
    }
    for entry in std::fs::read_dir(golden::selfmcp_root()).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = name.strip_suffix(".json").unwrap_or(&name);
        assert!(owned.contains(stem), "golden {name} belongs to no case");
    }
}

#[test]
fn selfmcp_coverage() {
    let world = World::new("coverage");
    let mut client = Client::spawn(&world, &[]);
    client.handshake();
    let listed: BTreeSet<String> = client
        .list_tools()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        listed,
        tool_names().iter().map(|n| n.to_string()).collect(),
        "tools/list and the catalog differ"
    );
    let served: BTreeSet<String> = client.request("resources/list", json!({}))["result"]
        ["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        served,
        RESOURCES.iter().map(|r| r.uri.to_string()).collect(),
        "resources/list and the catalog differ"
    );
    client.close();

    let covered: BTreeSet<&str> = CASES
        .iter()
        .filter(|case| {
            case.calls.iter().any(|c| {
                c.golden
                    && c.expect == Expect::Ok
                    && golden::read_in(&golden::selfmcp_root(), &stem(case, c)).is_some()
            })
        })
        .map(|case| case.tool)
        .collect();
    let missing: Vec<&str> = tool_names()
        .into_iter()
        .filter(|name| !covered.contains(name))
        .collect();
    let missing_resources: Vec<String> = RESOURCES
        .iter()
        .map(|r| resource_stem(r.uri))
        .filter(|stem| golden::read_in(&golden::selfmcp_root(), stem).is_none())
        .collect();
    eprintln!(
        "selfmcp contract: {} tools, {} with a golden result, {} uncovered",
        TOOLS.len(),
        TOOLS.len() - missing.len(),
        missing.len() + missing_resources.len(),
    );
    assert!(
        missing.is_empty(),
        "no golden result for tools: {missing:?}"
    );
    assert!(
        missing_resources.is_empty(),
        "no golden result for resources: {missing_resources:?}"
    );
}

/// The vault key is a live secret in the process: it must not show up in a result, a resource or a
/// golden. The client fails on the first line that holds it.
#[test]
fn no_output_carries_the_vault_key_or_a_server_secret() {
    for case in CASES {
        let world = World::new(case.tool);
        if let Some(prepare) = case.prepare {
            prepare(&world);
        }
        let mut client = Client::spawn(&world, &[FAKE_SECRET, SECRET_KEY]);
        client.handshake();
        for call in case.calls {
            let reply = client.call(call.tool, arguments(&world, case, call));
            let text = reply.0.to_string();
            for secret in [FAKE_SECRET, SECRET_KEY] {
                assert!(!text.contains(secret), "{} {}", case.tool, call.label);
            }
        }
        for def in RESOURCES {
            let content = client.read(def.uri);
            let text = content.to_string();
            for secret in [FAKE_SECRET, SECRET_KEY] {
                assert!(!text.contains(secret), "{}", def.uri);
            }
        }
        client.close();
    }
    for entry in std::fs::read_dir(golden::selfmcp_root()).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        for secret in [FAKE_SECRET, SECRET_KEY] {
            assert!(
                !text.contains(secret),
                "golden {} contains a secret value",
                entry.path().display()
            );
        }
    }
}
