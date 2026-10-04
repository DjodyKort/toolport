use super::{backend, compression, content, context, direct, plugins, servers, skills, sources, state, ToolError};
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    None,
    Always,
    UnlessDryRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Str,
    Bool,
    Obj,
    StrList,
    Int,
}

pub struct Param {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
    pub desc: &'static str,
    pub default: Option<bool>,
}

pub type Runner = fn(&Value) -> Result<Value, ToolError>;

pub struct ToolDef {
    pub name: &'static str,
    pub tier: u8,
    pub gate: Gate,
    pub description: &'static str,
    pub params: &'static [Param],
    pub run: Runner,
}

pub struct ResourceDef {
    pub uri: &'static str,
    pub name: &'static str,
    pub mime: &'static str,
    pub description: &'static str,
}

const fn p(name: &'static str, ty: Ty, required: bool, desc: &'static str) -> Param {
    Param {
        name,
        ty,
        required,
        desc,
        default: None,
    }
}

const fn on_by_default(param: Param) -> Param {
    Param {
        default: Some(true),
        ..param
    }
}

const REPO: Param = p("repo_path", Ty::Str, false, "Skills repository root");
const NAME: Param = p("name", Ty::Str, true, "Entity or server name");
const CLIENTS: Param = p("client_keys", Ty::StrList, false, "Client keys to target");
const DRY: Param = p("dry_run", Ty::Bool, false, "Report without writing");
const BODY: Param = p("new_body", Ty::Str, true, "Replacement body");
const DRY_ON: Param = on_by_default(p(
    "dry_run",
    Ty::Bool,
    false,
    "Report without writing; true unless you pass false to apply",
));
const COMMIT: Param = p("commit_message", Ty::Str, true, "Commit message");
const CLIENT: Param = p("client", Ty::Str, false, "Limit to one client key");
const GLOBAL: Param = p(
    "global_mode",
    Ty::Bool,
    false,
    "User-level locations, the default; false works inside the repository",
);

macro_rules! tool {
    ($name:literal, $tier:literal, $gate:ident, $desc:literal, [$($param:expr),* $(,)?], $run:path) => {
        ToolDef {
            name: $name,
            tier: $tier,
            gate: Gate::$gate,
            description: $desc,
            params: &[$($param),*],
            run: $run,
        }
    };
}

pub const TOOLS: &[ToolDef] = &[
    tool!(
        "skills_list",
        1,
        None,
        "List skills and rules in the repository",
        [REPO],
        skills::list
    ),
    tool!(
        "sources_ls",
        1,
        None,
        "List where skills, commands, agents, rules and CLAUDE.md files come from: repos, clients, plugins, the org sync, the library and loose files",
        [
            p("source", Ty::Str, false, "Only this source id, detector or origin kind"),
            p("kind", Ty::Str, false, "Only items of this kind: skill, command, agent, rule or memory"),
            p("items", Ty::Bool, false, "Include the items of every source"),
            p("cwd", Ty::Str, false, "Add the repository around this directory to the scan"),
            p("deep", Ty::Bool, false, "Double the depth and time budgets"),
            p("refresh", Ty::Bool, false, "Ignore the scan cache")
        ],
        sources::ls
    ),
    tool!(
        "context_measure",
        2,
        None,
        "Measure what a folder really loads, in tokens, by asking Claude Code itself: one request per variant, which spends model tokens and is cached per Claude Code version, model and settings. Returns the as-is total, the signed deltas of each without, and the skills the model is not offered",
        [
            p("cwd", Ty::Str, true, "The folder to measure"),
            p("without", Ty::StrList, false, "Variants to measure besides as-is: plugin:<id> or skill:<name or glob>"),
            p("bundle", Ty::Str, false, "A bundle from the skills repository's profiles to measure as one variant"),
            p("model", Ty::Str, false, "Model to ask, haiku by default"),
            p("force", Ty::Bool, false, "Measure again instead of answering from the cache")
        ],
        context::measure_tool
    ),
    tool!(
        "plugins_ls",
        1,
        None,
        "List the installed Claude Code plugins with what each brings (skills, agents, commands, hooks, MCP servers), whether it is on in the folder, its token cost and its MCP servers that sit outside the gateway",
        [
            p("cwd", Ty::Str, false, "Folder where Claude Code starts; its project and local settings decide which plugins are on"),
            p("refresh", Ty::Bool, false, "Run claude plugin marketplace update first")
        ],
        plugins::ls
    ),
    tool!(
        "plugins_show",
        1,
        None,
        "Show one installed plugin: components, hooks, MCP servers with their deny state, options and token cost. Option values marked sensitive are never returned",
        [
            p("id", Ty::Str, true, "Plugin id such as ecc@ecc, or a bare plugin name when it is unique"),
            p("cwd", Ty::Str, false, "Folder where Claude Code starts; its project and local settings decide whether the plugin is on and its MCP servers denied")
        ],
        plugins::show
    ),
    tool!(
        "hooks_ls",
        1,
        None,
        "List every hook Claude Code would start in a folder, read from settings, plugins and Toolport files; a hook is never run. Counts the processes per tool and flags PreToolUse hooks of different owners on the same tool",
        [
            p("cwd", Ty::Str, false, "Folder where Claude Code starts; without it only user and managed settings apply"),
            p("tool", Ty::Str, false, "Only hooks that fire for this tool: Bash, Edit, Write or Read"),
            p("event", Ty::Str, false, "Only hooks on this event, for example PreToolUse"),
            p("owner", Ty::Str, false, "Only hooks of this owner kind: plugin, user, project, local, skill, toolport or managed")
        ],
        plugins::hooks_ls
    ),
    tool!(
        "skills_get",
        1,
        None,
        "Read one skill including its body",
        [NAME, REPO],
        skills::get
    ),
    tool!(
        "skills_lint",
        1,
        None,
        "Lint skills",
        [
            REPO,
            p("names", Ty::StrList, false, "Limit to these skills")
        ],
        skills::lint
    ),
    tool!(
        "skills_status",
        1,
        None,
        "Drift between repository and client outputs",
        [REPO, CLIENTS],
        skills::status
    ),
    tool!(
        "skills_list_transpilers",
        1,
        None,
        "List client transpilers",
        [],
        skills::list_transpilers
    ),
    tool!(
        "skills_scaffold",
        2,
        None,
        "Create a new skill skeleton",
        [NAME, p("skill_type", Ty::Str, false, "skill or rule"), REPO],
        content::skills_scaffold
    ),
    tool!(
        "skills_sync",
        2,
        None,
        "Transpile skills to client outputs",
        [
            REPO,
            CLIENTS,
            DRY,
            p(
                "global_mode",
                Ty::Bool,
                false,
                "Write to user-level locations"
            ),
            p(
                "migrate",
                Ty::Bool,
                false,
                "Back up and replace files that shadow a synced skill; otherwise they are only reported"
            )
        ],
        skills::sync
    ),
    tool!(
        "skills_tap_list",
        1,
        None,
        "List the registered skill taps (git sources) and whether each is cloned",
        [],
        content::skills_tap_list
    ),
    tool!(
        "skills_search",
        1,
        None,
        "Search the cloned taps for skills by name, description or tags",
        [p("query", Ty::Str, true, "Text to look for")],
        content::skills_search
    ),
    tool!(
        "skills_tap_add",
        2,
        None,
        "Register a tap and clone it; dry_run is on by default",
        [
            p("repo", Ty::Str, true, "user/repo on GitHub, or an https, ssh or file git URL"),
            p("name", Ty::Str, false, "Tap name, default derived from the source"),
            DRY_ON
        ],
        content::skills_tap_add
    ),
    tool!(
        "skills_tap_remove",
        2,
        None,
        "Unregister a tap and delete its clone; dry_run is on by default",
        [NAME, DRY_ON],
        content::skills_tap_remove
    ),
    tool!(
        "skills_tap_update",
        2,
        None,
        "Pull one tap or all of them; dry_run is on by default",
        [p("name", Ty::Str, false, "Limit to one tap"), DRY_ON],
        content::skills_tap_update
    ),
    tool!(
        "skills_install",
        2,
        None,
        "Install the skills of @user/repo[/skill] from a tap into the skills repository after a security audit; dry_run is on by default",
        [
            p("spec", Ty::Str, true, "@user/repo, @user/repo/skill or with @version"),
            REPO,
            DRY_ON
        ],
        content::skills_install
    ),
    tool!(
        "skills_diff",
        1,
        None,
        "Compare the skills with the lockfile: new, modified, removed and unchanged",
        [REPO],
        skills::diff
    ),
    tool!(
        "skills_audit",
        1,
        None,
        "Scan skills for prompt injection and risky commands",
        [REPO],
        state::skills_audit
    ),
    tool!(
        "skills_bundle",
        2,
        None,
        "Pack skills into a new .zip bundle; never overwrites a file; dry_run is on by default",
        [
            REPO,
            p("output", Ty::Str, false, "Path of the .zip to create, default <repository folder>-bundle.zip inside the repository"),
            p("skills", Ty::StrList, false, "Limit to these skills"),
            DRY_ON
        ],
        state::skills_bundle
    ),
    tool!(
        "skills_unbundle",
        3,
        UnlessDryRun,
        "Extract a skills bundle into the repository, overwriting files of the same name; refuses a bundle with files outside skills/ and rules/; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("bundle_path", Ty::Str, true, "Path of the .zip bundle"),
            REPO,
            DRY_ON
        ],
        state::skills_unbundle
    ),
    tool!(
        "skills_clean",
        4,
        UnlessDryRun,
        "Remove the synced skill outputs and the lockfile; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [REPO, CLIENT, GLOBAL, DRY_ON],
        state::skills_clean
    ),
    tool!(
        "skills_uninstall",
        4,
        UnlessDryRun,
        "Delete one skill or rule from the repository with its synced outputs and lock entry; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [NAME, REPO, GLOBAL, DRY_ON],
        state::skills_uninstall
    ),
    tool!(
        "skills_resolve",
        3,
        UnlessDryRun,
        "Find files that shadow synced skills; migrate=true backs them up and replaces them, otherwise they are only reported; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            REPO,
            CLIENT,
            GLOBAL,
            p("migrate", Ty::Bool, false, "Back up and replace the shadowing files"),
            DRY_ON
        ],
        state::skills_resolve
    ),
    tool!(
        "skills_edit_body",
        3,
        Always,
        "Replace a skill body in the canonical repository",
        [NAME, BODY, REPO],
        content::skills_edit_body
    ),
    tool!(
        "skills_edit_frontmatter",
        3,
        Always,
        "Patch a skill's frontmatter",
        [
            NAME,
            p("patch", Ty::Obj, true, "Frontmatter fields to set"),
            REPO
        ],
        content::skills_edit_frontmatter
    ),
    tool!(
        "skills_delete",
        3,
        Always,
        "Delete a skill from the canonical repository",
        [NAME, REPO],
        content::skills_delete
    ),
    tool!(
        "skills_git_push",
        4,
        Always,
        "Commit and push the skills repository to its remote",
        [COMMIT, REPO],
        servers::skills_git_push
    ),
    tool!("agents_list", 1, None, "List agents", [REPO], content::agents_list),
    tool!(
        "agents_get",
        1,
        None,
        "Read one agent including its body",
        [NAME, REPO],
        content::agents_get
    ),
    tool!("agents_lint", 1, None, "Lint agents", [REPO], content::agents_lint),
    tool!(
        "agents_list_transpilers",
        1,
        None,
        "List agent transpilers",
        [],
        content::agents_list_transpilers
    ),
    tool!(
        "agents_scaffold",
        2,
        None,
        "Create a new agent skeleton",
        [
            NAME,
            p("model", Ty::Str, false, "Model, default inherit"),
            REPO
        ],
        content::agents_scaffold
    ),
    tool!(
        "agents_sync",
        2,
        None,
        "Transpile agents to client outputs",
        [
            REPO,
            CLIENTS,
            DRY,
            p(
                "global_mode",
                Ty::Bool,
                false,
                "Write to user-level locations"
            )
        ],
        content::agents_sync
    ),
    tool!(
        "agents_diff",
        1,
        None,
        "Compare the agents with the lockfile: new, modified, removed and unchanged",
        [REPO],
        state::agents_diff
    ),
    tool!(
        "agents_audit",
        1,
        None,
        "Scan agents for prompt injection and risky commands",
        [REPO],
        state::agents_audit
    ),
    tool!(
        "agents_status",
        1,
        None,
        "Show which synced agent outputs are present for each client",
        [REPO],
        state::agents_status
    ),
    tool!(
        "agents_clean",
        4,
        UnlessDryRun,
        "Remove the synced agent outputs; the lockfile stays; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [REPO, CLIENT, GLOBAL, DRY_ON],
        state::agents_clean
    ),
    tool!(
        "agents_uninstall",
        4,
        UnlessDryRun,
        "Delete one agent from the repository with its synced outputs and lock entry; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [NAME, REPO, GLOBAL, DRY_ON],
        state::agents_uninstall
    ),
    tool!(
        "agents_edit_body",
        3,
        Always,
        "Replace an agent body in the canonical repository",
        [NAME, BODY, REPO],
        content::agents_edit_body
    ),
    tool!("styles_list", 1, None, "List styles", [REPO], content::styles_list),
    tool!(
        "styles_get",
        1,
        None,
        "Read one style including its body",
        [NAME, REPO],
        content::styles_get
    ),
    tool!("styles_lint", 1, None, "Lint styles", [REPO], content::styles_lint),
    tool!(
        "styles_active",
        1,
        None,
        "Show the applied style per client",
        [REPO],
        content::styles_active
    ),
    tool!(
        "styles_list_transpilers",
        1,
        None,
        "List style transpilers",
        [],
        content::styles_list_transpilers
    ),
    tool!(
        "styles_scaffold",
        2,
        None,
        "Create a new style skeleton",
        [NAME, REPO],
        content::styles_scaffold
    ),
    tool!(
        "styles_sync_tier1",
        2,
        None,
        "Sync tier-1 style outputs",
        [REPO, CLIENTS, DRY],
        content::styles_sync_tier1
    ),
    tool!(
        "styles_apply",
        3,
        Always,
        "Apply a style to tier-2 clients",
        [NAME, REPO, CLIENTS, DRY],
        content::styles_apply
    ),
    tool!(
        "styles_diff",
        1,
        None,
        "Compare the styles with the lockfile: new, modified, removed and unchanged",
        [REPO],
        state::styles_diff
    ),
    tool!(
        "styles_status",
        1,
        None,
        "Show the styles synced to native clients and the applied style per client",
        [REPO],
        state::styles_status
    ),
    tool!(
        "styles_clean",
        4,
        UnlessDryRun,
        "Remove the synced style outputs and clear the applied styles; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [REPO, GLOBAL, DRY_ON],
        state::styles_clean
    ),
    tool!(
        "styles_edit_body",
        3,
        Always,
        "Replace a style body in the canonical repository",
        [NAME, BODY, REPO],
        content::styles_edit_body
    ),
    tool!(
        "styles_remove",
        4,
        Always,
        "Remove the applied style from tier-2 clients",
        [REPO, CLIENTS, DRY],
        content::styles_remove
    ),
    tool!(
        "compression_status",
        1,
        None,
        "Show the compression provider, active preset, engine pin and shims",
        [],
        compression::status
    ),
    tool!(
        "compression_enable",
        3,
        UnlessDryRun,
        "Enable compression: writes the policy config, the shell shims and the registry entry of the provider (default headroom); dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("provider", Ty::Str, false, "headroom, rtk-only, parsec or none"),
            p("port", Ty::Int, false, "Proxy port of the active preset"),
            p("telemetry", Ty::Str, false, "on or off"),
            p("preset", Ty::Str, false, "Preset to make active"),
            p("mode", Ty::Str, false, "cache or token"),
            DRY_ON
        ],
        compression::enable
    ),
    tool!(
        "compression_disable",
        4,
        UnlessDryRun,
        "Switch compression off (provider none) and remove the generated shims and registry entry; presets and the pin stay and the engine is not torn down; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [DRY_ON],
        compression::disable
    ),
    tool!(
        "compression_set_provider",
        3,
        UnlessDryRun,
        "Switch the compression provider and re-apply the policy; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("provider", Ty::Str, true, "headroom, rtk-only, parsec or none"),
            DRY_ON
        ],
        compression::set_provider
    ),
    tool!(
        "compression_use",
        3,
        UnlessDryRun,
        "Make a compression preset active and re-apply the policy; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [p("preset", Ty::Str, true, "Preset name"), DRY_ON],
        compression::use_preset
    ),
    tool!(
        "compression_sync",
        3,
        UnlessDryRun,
        "Re-apply the stored compression policy to the shims, the registry entry and the engine, adopting a legacy mcpm policy when none is stored; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [DRY_ON],
        compression::sync
    ),
    tool!(
        "compression_seal",
        3,
        UnlessDryRun,
        "Read the running proxy's /health and record the settings the policy never declared into the preset; needs a running proxy; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("preset", Ty::Str, false, "Preset to seal, default the active one"),
            DRY_ON
        ],
        compression::seal
    ),
    tool!(
        "servers_list",
        1,
        None,
        "List registered servers",
        [p(
            "include_sources",
            Ty::Bool,
            false,
            "Include source information"
        )],
        backend::servers_list
    ),
    tool!(
        "servers_get",
        1,
        None,
        "Read one server entry without secret values",
        [NAME],
        backend::servers_get
    ),
    tool!(
        "servers_list_profiles",
        1,
        None,
        "List profiles and their enabled servers",
        [],
        backend::servers_list_profiles
    ),
    tool!(
        "servers_detect_source",
        1,
        None,
        "Detect where a server was installed from",
        [NAME],
        servers::detect_source
    ),
    tool!(
        "servers_git_status",
        1,
        None,
        "Git status of a source-installed server",
        [NAME],
        servers::git_status
    ),
    tool!(
        "servers_check_updates",
        1,
        None,
        "Check servers for available updates",
        [
            p("name", Ty::Str, false, "Limit to one server"),
            p(
                "include_prerelease",
                Ty::Bool,
                false,
                "Consider prereleases"
            )
        ],
        servers::check_updates
    ),
    tool!(
        "servers_add_profile_tag",
        2,
        None,
        "Add a server to a profile",
        [NAME, p("profile_tag", Ty::Str, true, "Profile name")],
        servers::add_profile_tag
    ),
    tool!(
        "servers_remove_profile_tag",
        2,
        None,
        "Remove a server from a profile",
        [NAME, p("profile_tag", Ty::Str, true, "Profile name")],
        servers::remove_profile_tag
    ),
    tool!(
        "servers_install",
        3,
        Always,
        "Register a new server",
        [
            NAME,
            p("config", Ty::Obj, true, "Server configuration"),
            p("profile_tags", Ty::StrList, false, "Profiles to join"),
            p("force", Ty::Bool, false, "Replace an existing entry")
        ],
        servers::install
    ),
    tool!(
        "servers_update_config",
        3,
        Always,
        "Patch a server's configuration",
        [NAME, p("patch", Ty::Obj, true, "Fields to change")],
        servers::update_config
    ),
    tool!(
        "servers_apply_update",
        3,
        Always,
        "Apply an available server update",
        [
            NAME,
            p("rebase", Ty::Bool, false, "Rebase local changes"),
            p(
                "include_prerelease",
                Ty::Bool,
                false,
                "Consider prereleases"
            )
        ],
        servers::apply_update
    ),
    tool!(
        "servers_set_mode",
        3,
        Always,
        "Change how a server is exposed to clients",
        [
            NAME,
            p(
                "mode",
                Ty::Str,
                true,
                "auto, direct, router, legacy or bridge"
            )
        ],
        servers::set_mode
    ),
    tool!(
        "servers_fork_sync",
        3,
        Always,
        "Sync a forked source server with its upstream",
        [
            NAME,
            p("upstream_remote", Ty::Str, false, "Default upstream"),
            p("upstream_branch", Ty::Str, false, "Default main"),
            p("mode", Ty::Str, false, "Default rebase"),
            p("author_email", Ty::Str, false, "Commit author email"),
            p("target_branch", Ty::Str, false, "Branch to update"),
            p(
                "run_post_update",
                Ty::Bool,
                false,
                "Run the post-update command"
            )
        ],
        servers::fork_sync
    ),
    tool!(
        "servers_auth",
        3,
        Always,
        "Start the authorization flow for a server",
        [NAME],
        servers::auth
    ),
    tool!(
        "servers_uninstall",
        4,
        Always,
        "Remove a server and its client entries",
        [
            NAME,
            p(
                "propagate_to_clients",
                Ty::Bool,
                false,
                "Remove client entries too"
            )
        ],
        servers::uninstall
    ),
    tool!("clients_list", 1, None, "List supported client keys", [], backend::clients_list),
    tool!(
        "clients_sync",
        2,
        UnlessDryRun,
        "Reconcile client configs with the registry",
        [
            p("client", Ty::Str, false, "Single client key"),
            p("safe", Ty::Bool, false, "Skip risky rewrites"),
            p(
                "force_legacy",
                Ty::Bool,
                false,
                "Use the legacy entry shape"
            ),
            p("keep_orphans", Ty::Bool, false, "Keep unmatched entries"),
            DRY
        ],
        servers::clients_sync
    ),
    tool!(
        "client_direct_ls",
        1,
        None,
        "List direct client entries that start one server through the stdio launcher, bypassing the gateway",
        [p("client", Ty::Str, false, "Limit to one client key")],
        direct::client_direct_ls
    ),
    tool!(
        "client_direct_add",
        2,
        None,
        "Add an entry for one stdio server to one client that runs the toolportctl launcher instead of the gateway; it bypasses profile tool scopes, approvals, receipts and lazy discovery, and each client starts its own process; dry_run is on by default",
        [
            p("server", Ty::Str, true, "Server name or id"),
            p("client", Ty::Str, true, "Client key"),
            p("force", Ty::Bool, false, "Replace an entry this tool did not create"),
            DRY_ON
        ],
        direct::client_direct_add
    ),
    tool!(
        "client_direct_rm",
        2,
        None,
        "Remove the direct entry of one server from one client; dry_run is on by default",
        [
            p("server", Ty::Str, true, "Server name or id"),
            p("client", Ty::Str, true, "Client key"),
            p("force", Ty::Bool, false, "Remove an entry that was changed after it was written"),
            DRY_ON
        ],
        direct::client_direct_rm
    ),
    tool!(
        "sync_push",
        4,
        UnlessDryRun,
        "Publish the encrypted sync bundle to its remote",
        [DRY],
        servers::sync_push
    ),
    tool!(
        "where_am_i",
        1,
        None,
        "Absolute paths and state of this installation",
        [],
        backend::where_am_i
    ),
    tool!("doctor", 1, None, "Read-only health checks", [], backend::doctor),
    tool!(
        "flow_diagram",
        1,
        None,
        "Data-flow diagram of skills and servers",
        [],
        backend::flow_diagram
    ),
];

pub const RESOURCES: &[ResourceDef] = &[
    ResourceDef {
        uri: "mcpm://paths",
        name: "paths",
        mime: "application/json",
        description: "Canonical, sync and output roots",
    },
    ResourceDef {
        uri: "mcpm://status",
        name: "status",
        mime: "application/json",
        description: "Registry summary and drift counts",
    },
    ResourceDef {
        uri: "mcpm://flow",
        name: "flow",
        mime: "text/markdown",
        description: "Data-flow diagram",
    },
    ResourceDef {
        uri: "mcpm://inventory/skills",
        name: "inventory-skills",
        mime: "text/plain",
        description: "Skills, name and description",
    },
    ResourceDef {
        uri: "mcpm://inventory/agents",
        name: "inventory-agents",
        mime: "text/plain",
        description: "Agents, name and description",
    },
    ResourceDef {
        uri: "mcpm://inventory/styles",
        name: "inventory-styles",
        mime: "text/plain",
        description: "Styles, name and description",
    },
    ResourceDef {
        uri: "mcpm://inventory/servers",
        name: "inventory-servers",
        mime: "text/plain",
        description: "Servers, transport and source",
    },
    ResourceDef {
        uri: "mcpm://clients",
        name: "clients",
        mime: "application/json",
        description: "Supported client keys",
    },
    ResourceDef {
        uri: "mcpm://architecture",
        name: "architecture",
        mime: "text/markdown",
        description: "Architecture reference",
    },
    ResourceDef {
        uri: "mcpm://workflows",
        name: "workflows",
        mime: "text/markdown",
        description: "Recipes for multi-step operations",
    },
    ResourceDef {
        uri: "mcpm://router/status",
        name: "router-status",
        mime: "application/json",
        description: "Live gateway state",
    },
];

pub fn find_tool(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.name == name)
}

pub fn find_resource(uri: &str) -> Option<&'static ResourceDef> {
    RESOURCES.iter().find(|r| r.uri == uri)
}

fn ty_schema(ty: Ty) -> Value {
    match ty {
        Ty::Str => json!({"type": "string"}),
        Ty::Bool => json!({"type": "boolean"}),
        Ty::Obj => json!({"type": "object"}),
        Ty::StrList => json!({"type": "array", "items": {"type": "string"}}),
        Ty::Int => json!({"type": "integer"}),
    }
}

pub fn input_schema(tool: &ToolDef) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for param in tool.params {
        let mut schema = ty_schema(param.ty);
        schema["description"] = json!(param.desc);
        if let Some(default) = param.default {
            schema["default"] = json!(default);
        }
        props.insert(param.name.to_string(), schema);
        if param.required {
            required.push(json!(param.name));
        }
    }
    if tool.gate != Gate::None {
        props.insert(
            "confirm".to_string(),
            json!({"type": "boolean", "default": false, "description": "Must be true to run this tool"}),
        );
    }
    let mut schema = json!({
        "type": "object",
        "properties": props,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

pub fn tier_label(tier: u8) -> &'static str {
    match tier {
        1 => "read-only",
        2 => "write",
        3 => "confirm",
        _ => "destructive",
    }
}

pub fn tool_descriptor(tool: &ToolDef) -> Value {
    json!({
        "name": tool.name,
        "description": format!("[tier {} {}] {}", tool.tier, tier_label(tool.tier), tool.description),
        "inputSchema": input_schema(tool),
        "annotations": {
            "readOnlyHint": tool.tier == 1,
            "destructiveHint": tool.tier >= 4,
        },
    })
}
