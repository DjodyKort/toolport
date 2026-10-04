//! The table of `selfmcp_envelopes.rs`: one case per catalog tool. A tool with a `dry_run`
//! parameter previews first (`read`), a gated one is refused without `confirm` (`refused`) and then
//! runs with it (`write`); a tool that takes a name fails for an unknown or a tampered one.
//! `setup` calls build the state a tool acts on and record no golden.

use crate::ctl_fixtures::{
    fork_back_to_main, fork_world, git_world, health_proxy, skills_repo_remote_world, sync_setup,
};

use super::{case, fails, hook, prepared, read, refused, setup, write, Case};

const CLAUDE: &str = r#"{"client_keys":["claude-code"]}"#;
const CLAUDE_PREVIEW: &str = r#"{"client_keys":["claude-code"],"dry_run":true}"#;
const AIDER: &str = r#"{"name":"plain","client_keys":["aider"],"confirm":true}"#;
const AIDER_PREVIEW_CONFIRMED: &str = r#"{"client_keys":["aider"],"dry_run":true,"confirm":true}"#;
const AIDER_REMOVE_UNCONFIRMED: &str = r#"{"client_keys":["aider"],"dry_run":false}"#;
const AIDER_REMOVE: &str = r#"{"client_keys":["aider"],"dry_run":false,"confirm":true}"#;
const APPLY: &str = r#"{"dry_run":false,"confirm":true}"#;
const UNCONFIRMED: &str = r#"{"dry_run":false}"#;
const ADD_TAP: &str = r#"{"repo":"{base}/tap-src","name":"local","dry_run":false}"#;
const DIRECT: &str = r#"{"server":"alpha","client":"claude-code"}"#;
const DIRECT_APPLY: &str = r#"{"server":"alpha","client":"claude-code","dry_run":false}"#;
const GAMMA: &str = r#"{"name":"gamma","config":{"command":"gamma-mcp","args":["--flag"]}}"#;
const GAMMA_APPLY: &str =
    r#"{"name":"gamma","config":{"command":"gamma-mcp","args":["--flag"]},"confirm":true}"#;
const BUNDLE: &str = r#"{"bundle_path":"{repo}/skills-repo-bundle.zip"}"#;
const ENABLE_PROXY_PORT: &str =
    r#"{"provider":"headroom","port":49214,"dry_run":false,"confirm":true}"#;
const ENABLE_OFF_PORT: &str =
    r#"{"provider":"headroom","port":49213,"dry_run":false,"confirm":true}"#;

fn proxy_world(_: &crate::CtlWorld) {
    health_proxy(49214);
}

pub const ALL: &[Case] = &[
    // skills
    case("skills_list", &[read("", "skills_list", "{}")]),
    case(
        "skills_get",
        &[
            read("found", "skills_get", r#"{"name":"demo"}"#),
            fails("missing", "not_found", "skills_get", r#"{"name":"nope"}"#),
        ],
    ),
    case("skills_lint", &[read("", "skills_lint", "{}")]),
    case(
        "skills_status",
        &[
            read("fresh", "skills_status", "{}"),
            setup("skills_sync", CLAUDE),
            read("synced", "skills_status", CLAUDE),
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
            read("preview", "skills_sync", CLAUDE_PREVIEW),
            write("apply", "skills_sync", CLAUDE),
            read(
                "unknown_client",
                "skills_sync",
                r#"{"client_keys":["no-such-client"]}"#,
            ),
        ],
    ),
    prepared(
        "skills_tap_list",
        git_world,
        &[
            read("empty", "skills_tap_list", "{}"),
            setup("skills_tap_add", ADD_TAP),
            read("with_tap", "skills_tap_list", "{}"),
        ],
    ),
    prepared(
        "skills_search",
        git_world,
        &[
            read("empty", "skills_search", r#"{"query":"tapskill"}"#),
            setup("skills_tap_add", ADD_TAP),
            read("hit", "skills_search", r#"{"query":"tapskill"}"#),
        ],
    ),
    prepared(
        "skills_tap_add",
        git_world,
        &[
            read("preview", "skills_tap_add", r#"{"repo":"acme/skills"}"#),
            write("apply", "skills_tap_add", ADD_TAP),
            fails(
                "escaping_name",
                "invalid_arguments",
                "skills_tap_add",
                r#"{"repo":"acme/skills","name":"../x"}"#,
            ),
        ],
    ),
    prepared(
        "skills_tap_remove",
        git_world,
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
    prepared(
        "skills_tap_update",
        git_world,
        &[
            read("none", "skills_tap_update", "{}"),
            setup("skills_tap_add", ADD_TAP),
            read("preview", "skills_tap_update", "{}"),
            write("apply", "skills_tap_update", r#"{"dry_run":false}"#),
        ],
    ),
    // an `@user/repo` spec clones from github, so only the dry-run plan is recorded
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
    case(
        "skills_diff",
        &[
            read("fresh", "skills_diff", "{}"),
            setup("skills_sync", CLAUDE),
            read("synced", "skills_diff", "{}"),
        ],
    ),
    case("skills_audit", &[read("", "skills_audit", "{}")]),
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
            setup("skills_sync", CLAUDE),
            read("preview", "skills_clean", "{}"),
            refused("refused", "skills_clean", UNCONFIRMED),
            write("apply", "skills_clean", APPLY),
        ],
    ),
    case(
        "skills_uninstall",
        &[
            setup("skills_sync", CLAUDE),
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
            setup("skills_sync", CLAUDE),
            read("preview", "skills_resolve", "{}"),
            refused("refused", "skills_resolve", UNCONFIRMED),
            write("apply", "skills_resolve", APPLY),
        ],
    ),
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
    prepared(
        "skills_git_push",
        skills_repo_remote_world,
        &[
            refused(
                "refused",
                "skills_git_push",
                r#"{"commit_message":"add extra"}"#,
            ),
            fails(
                "empty_message",
                "invalid_arguments",
                "skills_git_push",
                r#"{"commit_message":" ","confirm":true}"#,
            ),
            fails(
                "not_a_repository",
                "not_found",
                "skills_git_push",
                r#"{"commit_message":"x","repo_path":"{data}","confirm":true}"#,
            ),
            write(
                "apply",
                "skills_git_push",
                r#"{"commit_message":"add extra","confirm":true}"#,
            ),
            read(
                "clean",
                "skills_git_push",
                r#"{"commit_message":"again","confirm":true}"#,
            ),
        ],
    ),
    // agents
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
            read("preview", "agents_sync", CLAUDE_PREVIEW),
            write("apply", "agents_sync", CLAUDE),
        ],
    ),
    case(
        "agents_diff",
        &[
            read("fresh", "agents_diff", "{}"),
            setup("agents_sync", CLAUDE),
            read("synced", "agents_diff", "{}"),
        ],
    ),
    case("agents_audit", &[read("", "agents_audit", "{}")]),
    case(
        "agents_status",
        &[
            read("fresh", "agents_status", "{}"),
            setup("agents_sync", CLAUDE),
            read("synced", "agents_status", "{}"),
        ],
    ),
    case(
        "agents_clean",
        &[
            setup("agents_sync", CLAUDE),
            read("preview", "agents_clean", "{}"),
            refused("refused", "agents_clean", UNCONFIRMED),
            write("apply", "agents_clean", APPLY),
        ],
    ),
    case(
        "agents_uninstall",
        &[
            setup("agents_sync", CLAUDE),
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
            read("preview", "styles_sync_tier1", CLAUDE_PREVIEW),
            write("apply", "styles_sync_tier1", CLAUDE),
        ],
    ),
    case(
        "styles_apply",
        &[
            read(
                "preview",
                "styles_apply",
                r#"{"name":"plain","client_keys":["aider"],"dry_run":true,"confirm":true}"#,
            ),
            refused(
                "refused",
                "styles_apply",
                r#"{"name":"plain","client_keys":["aider"]}"#,
            ),
            write(
                "apply",
                "styles_apply",
                r#"{"name":"plain","client_keys":["aider"],"confirm":true}"#,
            ),
            fails(
                "missing",
                "not_found",
                "styles_apply",
                r#"{"name":"nope","client_keys":["aider"],"confirm":true}"#,
            ),
        ],
    ),
    case(
        "styles_diff",
        &[
            read("fresh", "styles_diff", "{}"),
            setup("styles_sync_tier1", CLAUDE),
            read("synced", "styles_diff", "{}"),
        ],
    ),
    case(
        "styles_status",
        &[
            read("fresh", "styles_status", "{}"),
            setup("styles_sync_tier1", CLAUDE),
            read("synced", "styles_status", "{}"),
        ],
    ),
    case(
        "styles_clean",
        &[
            setup("styles_sync_tier1", CLAUDE),
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
            read("preview", "styles_remove", AIDER_PREVIEW_CONFIRMED),
            refused("refused", "styles_remove", AIDER_REMOVE_UNCONFIRMED),
            write("apply", "styles_remove", AIDER_REMOVE),
        ],
    ),
    // compression
    case(
        "compression_status",
        &[
            read("fresh", "compression_status", "{}"),
            setup("compression_enable", ENABLE_OFF_PORT),
            read("enabled", "compression_status", "{}"),
        ],
    ),
    case(
        "compression_enable",
        &[
            read(
                "preview",
                "compression_enable",
                r#"{"provider":"rtk-only","port":49213,"mode":"cache"}"#,
            ),
            refused(
                "refused",
                "compression_enable",
                r#"{"provider":"rtk-only","port":49213,"mode":"cache","dry_run":false}"#,
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
                r#"{"port":"49213","dry_run":false,"confirm":true}"#,
            ),
            write(
                "apply",
                "compression_enable",
                r#"{"provider":"rtk-only","port":49213,"mode":"cache","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "compression_disable",
        &[
            setup("compression_enable", ENABLE_OFF_PORT),
            read("preview", "compression_disable", "{}"),
            refused("refused", "compression_disable", UNCONFIRMED),
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
    case(
        "compression_sync",
        &[
            read("preview", "compression_sync", "{}"),
            refused("refused", "compression_sync", UNCONFIRMED),
            write("apply", "compression_sync", APPLY),
        ],
    ),
    // seal reads the proxy's /health, even for a preview: first a port nothing listens on, then
    // one that answers like the proxy
    prepared(
        "compression_seal",
        proxy_world,
        &[
            setup("compression_enable", ENABLE_OFF_PORT),
            refused("refused", "compression_seal", UNCONFIRMED),
            fails("no_proxy", "backend_error", "compression_seal", APPLY),
            setup("compression_enable", ENABLE_PROXY_PORT),
            read("preview", "compression_seal", "{}"),
            write("apply", "compression_seal", APPLY),
            read("sealed", "compression_seal", APPLY),
        ],
    ),
    // servers
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
    prepared(
        "servers_detect_source",
        fork_world,
        &[
            read("unknown", "servers_detect_source", r#"{"name":"alpha"}"#),
            read("git", "servers_detect_source", r#"{"name":"forked"}"#),
        ],
    ),
    prepared(
        "servers_git_status",
        fork_world,
        &[
            read("not_git", "servers_git_status", r#"{"name":"alpha"}"#),
            read("git", "servers_git_status", r#"{"name":"forked"}"#),
        ],
    ),
    case(
        "servers_check_updates",
        &[
            read("all", "servers_check_updates", "{}"),
            read("one", "servers_check_updates", r#"{"name":"alpha"}"#),
        ],
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
            fails(
                "missing",
                "not_found",
                "servers_update_config",
                r#"{"name":"nope","patch":{"args":[]},"confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_apply_update",
        &[
            refused("refused", "servers_apply_update", r#"{"name":"alpha"}"#),
            read(
                "apply",
                "servers_apply_update",
                r#"{"name":"alpha","confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_set_mode",
        &[
            refused(
                "refused",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"direct"}"#,
            ),
            fails(
                "unknown_mode",
                "invalid_arguments",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"bogus","confirm":true}"#,
            ),
            read(
                "apply",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"direct","confirm":true}"#,
            ),
        ],
    ),
    prepared(
        "servers_fork_sync",
        fork_world,
        &[
            refused("refused", "servers_fork_sync", r#"{"name":"forked"}"#),
            fails(
                "not_git_backed",
                "invalid_input",
                "servers_fork_sync",
                r#"{"name":"alpha","confirm":true}"#,
            ),
            fails(
                "missing_author",
                "invalid_arguments",
                "servers_fork_sync",
                r#"{"name":"forked","mode":"onto-author","confirm":true}"#,
            ),
            write(
                "rebase",
                "servers_fork_sync",
                r#"{"name":"forked","target_branch":"main-synced","confirm":true}"#,
            ),
            hook(fork_back_to_main),
            write(
                "onto_author",
                "servers_fork_sync",
                r#"{"name":"forked","mode":"onto-author","author_email":"fixture@example.invalid","target_branch":"main-picked","confirm":true}"#,
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
        ],
    ),
    case(
        "servers_uninstall",
        &[
            refused("refused", "servers_uninstall", r#"{"name":"beta"}"#),
            write(
                "apply",
                "servers_uninstall",
                r#"{"name":"beta","confirm":true}"#,
            ),
            fails(
                "gone",
                "not_found",
                "servers_uninstall",
                r#"{"name":"beta","confirm":true}"#,
            ),
        ],
    ),
    // clients
    case("clients_list", &[read("", "clients_list", "{}")]),
    case(
        "clients_sync",
        &[
            refused("refused", "clients_sync", "{}"),
            read("preview", "clients_sync", r#"{"dry_run":true}"#),
            fails(
                "unknown_client",
                "not_found",
                "clients_sync",
                r#"{"dry_run":true,"client":"no-such-client"}"#,
            ),
            read("apply", "clients_sync", r#"{"confirm":true}"#),
        ],
    ),
    case(
        "client_direct_ls",
        &[
            read("empty", "client_direct_ls", "{}"),
            setup("client_direct_add", DIRECT_APPLY),
            read(
                "with_entry",
                "client_direct_ls",
                r#"{"client":"claude-code"}"#,
            ),
        ],
    ),
    case(
        "client_direct_add",
        &[
            read("preview", "client_direct_add", DIRECT),
            write("apply", "client_direct_add", DIRECT_APPLY),
            read("unchanged", "client_direct_add", DIRECT_APPLY),
            fails(
                "http_server",
                "invalid_arguments",
                "client_direct_add",
                r#"{"server":"beta","client":"claude-code","dry_run":false}"#,
            ),
            fails(
                "client_missing",
                "invalid_arguments",
                "client_direct_add",
                r#"{"server":"alpha"}"#,
            ),
        ],
    ),
    case(
        "client_direct_rm",
        &[
            setup("client_direct_add", DIRECT_APPLY),
            read("preview", "client_direct_rm", DIRECT),
            write("apply", "client_direct_rm", DIRECT_APPLY),
        ],
    ),
    // sync
    prepared(
        "sync_push",
        git_world,
        &[
            refused("refused", "sync_push", "{}"),
            fails(
                "unconfigured",
                "backend_error",
                "sync_push",
                r#"{"dry_run":true}"#,
            ),
            hook(sync_setup),
            read("preview", "sync_push", r#"{"dry_run":true}"#),
            write("apply", "sync_push", r#"{"confirm":true}"#),
        ],
    ),
    // reads of the whole setup
    case("where_am_i", &[read("", "where_am_i", "{}")]),
    case("doctor", &[read("", "doctor", "{}")]),
    case("flow_diagram", &[read("", "flow_diagram", "{}")]),
];
