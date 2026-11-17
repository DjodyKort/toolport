use serde_json::{json, Map, Value};
use toolport_plus_api::Host;

use super::{
    backend, compression, content, context, library, servers, skills, sources, tasks, ToolError,
};

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

pub type Runner = fn(&dyn Host, &Value) -> Result<Value, ToolError>;

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

#[allow(dead_code)]
pub const fn p(name: &'static str, ty: Ty, required: bool, desc: &'static str) -> Param {
    Param {
        name,
        ty,
        required,
        desc,
        default: None,
    }
}

#[allow(dead_code)]
pub const fn on_by_default(param: Param) -> Param {
    Param {
        default: Some(true),
        ..param
    }
}

const NAME: Param = p("name", Ty::Str, true, "Server id or name");
const DRY: Param = p(
    "dry_run",
    Ty::Bool,
    false,
    "Report the plan and write nothing",
);
const CWD: Param = p("cwd", Ty::Str, true, "The folder where Claude Code starts");
const ENTITY: Param = p("name", Ty::Str, true, "Entity or server name");
const BODY: Param = p("new_body", Ty::Str, true, "Replacement body");
const SKILL_NAME: Param = p("name", Ty::Str, true, "Skill or rule name");
const REPO: Param = p("repo_path", Ty::Str, false, "Skills repository root");
const CLIENTS: Param = p("client_keys", Ty::StrList, false, "Client keys to target");
const COMMIT: Param = p("commit_message", Ty::Str, true, "Commit message");
const CLIENT: Param = p("client", Ty::Str, false, "Limit to one client key");
const GLOBAL: Param = p(
    "global_mode",
    Ty::Bool,
    false,
    "User-level locations, the default; false works inside the repository",
);
const DRY_ON: Param = on_by_default(p(
    "dry_run",
    Ty::Bool,
    false,
    "Report without writing; true unless you pass false to apply",
));

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
        "where_am_i",
        1,
        None,
        "Absolute paths and state of this installation",
        [],
        backend::where_am_i
    ),
    tool!(
        "doctor",
        1,
        None,
        "Read-only health checks",
        [],
        backend::doctor
    ),
    tool!(
        "flow_diagram",
        1,
        None,
        "Data-flow diagram of skills and servers",
        [],
        backend::flow_diagram
    ),
    tool!(
        "skills_list",
        1,
        None,
        "List skills and rules in the repository",
        [REPO],
        skills::list
    ),
    tool!(
        "skills_get",
        1,
        None,
        "Read one skill including its body",
        [SKILL_NAME, REPO],
        skills::get
    ),
    tool!(
        "skills_lint",
        1,
        None,
        "Lint skills, the files a sync leaves out and links that would not be installed",
        [
            REPO,
            p("names", Ty::StrList, false, "Limit to these skills")
        ],
        skills::lint
    ),
    tool!(
        "sources_ls",
        1,
        None,
        "List where skills, commands, agents, rules and CLAUDE.md files come from: repos, clients, the org sync, the library, taps and loose files",
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
        "skills_files",
        1,
        None,
        "Every file of one library skill with what the next sync does to it (copied, would-copy, differs, hand-edited, removed-on-next-sync, not-copied with its reason, not-yours), its links and the two description limits",
        [SKILL_NAME, REPO, CLIENTS, GLOBAL],
        skills::files
    ),
    tool!(
        "skills_fix",
        2,
        UnlessDryRun,
        "Apply the one safe lint fix: fix=chmod makes the hook scripts of skills executable; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("fix", Ty::Str, true, "The fix to apply: chmod"),
            REPO,
            p("names", Ty::StrList, false, "Limit to these skills"),
            DRY_ON
        ],
        skills::fix
    ),
    tool!(
        "skills_status",
        1,
        None,
        "Drift between repository and client outputs, with a file-by-file verification",
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
        [SKILL_NAME, p("skill_type", Ty::Str, false, "skill or rule"), REPO],
        skills::scaffold
    ),
    tool!(
        "skills_sync",
        2,
        UnlessDryRun,
        "Transpile skills to client outputs; dry_run is on by default (apply with dry_run=false and confirm=true). Removals, new hooks and files that differ wait unless apply_held=true",
        [
            REPO,
            CLIENTS,
            DRY_ON,
            GLOBAL,
            p(
                "migrate",
                Ty::Bool,
                false,
                "Back up and replace files that shadow a synced skill; otherwise they are only reported"
            ),
            p(
                "apply_held",
                Ty::Bool,
                false,
                "Also remove files that left the library, install new hooks and replace files that differ (after a backup)"
            ),
            p(
                "allow_tap_hooks",
                Ty::Bool,
                false,
                "With apply_held, also install the hooks of skills that came from a tap"
            ),
            p(
                "unattended",
                Ty::Bool,
                false,
                "The run a schedule makes: does nothing while auto-sync is paused and holds removals, new hooks and files that differ; not with apply_held or migrate"
            )
        ],
        skills::sync
    ),
    tool!(
        "skills_undo",
        2,
        UnlessDryRun,
        "Put back what the last skills_sync changed from its snapshot (the files it wrote or removed, the hooks it installed, the lock) and pause auto-sync; a file changed after the sync is skipped and listed as a conflict, and changed hooks refuse the whole undo. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [REPO, GLOBAL, DRY_ON],
        skills::undo
    ),
    tool!(
        "skills_verify",
        1,
        None,
        "Compare every file the lock records with the library copy: matched, total and each mismatch with its reason (missing, differs, mode)",
        [REPO, CLIENTS, GLOBAL],
        skills::verify
    ),
    tool!(
        "skills_auto_status",
        1,
        None,
        "Whether the unattended sync is scheduled (on or off, when it runs next, how its task last ran), whether it is paused and why (undo or manual), and what the last unattended run applied and held",
        [],
        skills::auto_status
    ),
    tool!(
        "skills_auto_on",
        2,
        None,
        "Schedule the unattended sync every morning by turning on the shipped task skills-auto-sync. Asks the person to approve it through the approval broker, because the schedule then runs on its own. Pass dry_run to see the change without making it",
        [p("dry_run", Ty::Bool, false, "Report the change only; nothing is asked and nothing changes")],
        skills::auto_on
    ),
    tool!(
        "skills_auto_off",
        2,
        None,
        "Stop scheduling the unattended sync by turning off the shipped task skills-auto-sync; asks nothing. Pass dry_run to see the change without making it",
        [p("dry_run", Ty::Bool, false, "Report the change only; nothing changes")],
        skills::auto_off
    ),
    tool!(
        "skills_auto_pause",
        2,
        None,
        "Pause the unattended sync until skills_auto_resume",
        [],
        skills::auto_pause
    ),
    tool!(
        "skills_auto_resume",
        2,
        None,
        "Let the unattended sync run again",
        [],
        skills::auto_resume
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
        skills::audit
    ),
    tool!(
        "skills_tap_list",
        1,
        None,
        "List the registered skill taps (git sources) and whether each is cloned",
        [],
        skills::tap_list
    ),
    tool!(
        "skills_search",
        1,
        None,
        "Search the cloned taps for skills by name, description or tags",
        [p("query", Ty::Str, true, "Text to look for")],
        skills::search
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
        skills::tap_add
    ),
    tool!(
        "skills_tap_remove",
        2,
        None,
        "Unregister a tap and delete its clone; dry_run is on by default",
        [NAME, DRY_ON],
        skills::tap_remove
    ),
    tool!(
        "skills_tap_update",
        2,
        None,
        "Pull one tap or all of them; dry_run is on by default",
        [p("name", Ty::Str, false, "Limit to one tap"), DRY_ON],
        skills::tap_update
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
        skills::install
    ),
    tool!(
        "skills_edit_body",
        3,
        Always,
        "Replace a skill body in the canonical repository",
        [ENTITY, BODY, REPO],
        content::skills_edit_body
    ),
    tool!(
        "skills_edit_frontmatter",
        3,
        Always,
        "Patch a skill's frontmatter",
        [
            ENTITY,
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
        [ENTITY, REPO],
        content::skills_delete
    ),
    tool!("agents_list", 1, None, "List agents", [REPO], content::agents_list),
    tool!(
        "agents_get",
        1,
        None,
        "Read one agent including its body",
        [ENTITY, REPO],
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
            ENTITY,
            p("model", Ty::Str, false, "Model, default inherit"),
            REPO
        ],
        content::agents_scaffold
    ),
    tool!(
        "agents_sync",
        2,
        UnlessDryRun,
        "Transpile agents to client outputs; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            REPO,
            CLIENTS,
            DRY_ON,
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
        skills::agents_diff
    ),
    tool!(
        "agents_audit",
        1,
        None,
        "Scan agents for prompt injection and risky commands",
        [REPO],
        skills::agents_audit
    ),
    tool!(
        "agents_status",
        1,
        None,
        "Show which synced agent outputs are present for each client",
        [REPO],
        skills::agents_status
    ),
    tool!(
        "agents_clean",
        4,
        UnlessDryRun,
        "Remove the synced agent outputs; the lockfile stays; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [REPO, CLIENT, GLOBAL, DRY_ON],
        skills::agents_clean
    ),
    tool!(
        "agents_uninstall",
        4,
        UnlessDryRun,
        "Delete one agent from the repository with its synced outputs and lock entry; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [ENTITY, REPO, GLOBAL, DRY_ON],
        skills::agents_uninstall
    ),
    tool!(
        "agents_edit_body",
        3,
        Always,
        "Replace an agent body in the canonical repository",
        [ENTITY, BODY, REPO],
        content::agents_edit_body
    ),
    tool!("styles_list", 1, None, "List styles", [REPO], content::styles_list),
    tool!(
        "styles_get",
        1,
        None,
        "Read one style including its body",
        [ENTITY, REPO],
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
        [ENTITY, REPO],
        content::styles_scaffold
    ),
    tool!(
        "styles_sync_tier1",
        2,
        UnlessDryRun,
        "Sync tier-1 style outputs; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [REPO, CLIENTS, DRY_ON],
        content::styles_sync_tier1
    ),
    tool!(
        "styles_apply",
        3,
        UnlessDryRun,
        "Apply a style to tier-2 clients; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [ENTITY, REPO, CLIENTS, DRY_ON],
        content::styles_apply
    ),
    tool!(
        "styles_diff",
        1,
        None,
        "Compare the styles with the lockfile: new, modified, removed and unchanged",
        [REPO],
        skills::styles_diff
    ),
    tool!(
        "styles_status",
        1,
        None,
        "Show the styles synced to native clients and the applied style per client",
        [REPO],
        skills::styles_status
    ),
    tool!(
        "styles_clean",
        4,
        UnlessDryRun,
        "Remove the synced style outputs and clear the applied styles; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [REPO, GLOBAL, DRY_ON],
        skills::styles_clean
    ),
    tool!(
        "styles_edit_body",
        3,
        Always,
        "Replace a style body in the canonical repository",
        [ENTITY, BODY, REPO],
        content::styles_edit_body
    ),
    tool!(
        "styles_remove",
        4,
        UnlessDryRun,
        "Remove the applied style from tier-2 clients; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [REPO, CLIENTS, DRY_ON],
        content::styles_remove
    ),
    tool!(
        "library_status",
        1,
        None,
        "Show the skills library against its remote: branch, commits ahead and behind, uncommitted files, how git authenticates (gh, a credential helper or ssh) and any other clone of the same remote with its own counts. Pass fetch to contact the remote and test the credentials; without it nothing touches the network. Read only",
        [p("fetch", Ty::Bool, false, "Contact the remote (ls-remote, then fetch) to refresh the counts and test the credentials")],
        library::status
    ),
    tool!(
        "library_pull",
        2,
        None,
        "Fast-forward the skills library from its remote (fetches first). Refuses over uncommitted changes and never merges or rewrites history. dry_run is on by default and only reports the plan; pass dry_run false to apply, and review true to also get the plan skills sync would carry out afterwards",
        [DRY_ON, p("review", Ty::Bool, false, "After a pull that applied, also return the dry-run plan of skills sync; nothing is synced")],
        library::pull
    ),
    tool!(
        "skills_git_push",
        4,
        Always,
        "Commit and push the skills repository to its remote",
        [COMMIT, REPO],
        library::git_push
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
        skills::bundle
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
        skills::unbundle
    ),
    tool!(
        "skills_clean",
        4,
        UnlessDryRun,
        "Remove the synced skill outputs and the lockfile; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [REPO, CLIENT, GLOBAL, DRY_ON],
        skills::clean
    ),
    tool!(
        "skills_uninstall",
        4,
        UnlessDryRun,
        "Delete one skill or rule from the repository with its synced outputs and lock entry; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [SKILL_NAME, REPO, GLOBAL, DRY_ON],
        skills::uninstall
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
        skills::resolve
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
        "compression_measure",
        1,
        None,
        "Four honest questions about compression, each with its population and what it cannot tell: where context tokens come from, what the hooks did, the billed outcome per epoch and a rework check; reads Claude Code transcripts and the hook counters, writes nothing",
        [
            p("min_turns", Ty::Int, false, "Sessions with fewer turns are left out of the billed outcome and the rework check (default 5)"),
            p("since", Ty::Str, false, "Only sessions that started on this date or later, e.g. 2026-11-01")
        ],
        compression::measure
    ),
    tool!(
        "compression_hooks_pause",
        2,
        None,
        "Pause the compression hooks Toolport wrapped: they answer nothing, so Claude Code runs without rtk and read-once. With for or until the pause ends by itself, without an app running; without either it lasts until compression_hooks_resume. Applies from the next session",
        [
            p("for", Ty::Str, false, "How long: 1d, 6h, 90m or 1d12h; at most 30 days"),
            p("until", Ty::Str, false, "When it ends: an RFC 3339 time, 2026-11-18T09:12 (UTC) or a date; at most 30 days ahead")
        ],
        compression::hooks_pause
    ),
    tool!(
        "compression_hooks_resume",
        2,
        None,
        "End a pause of the compression hooks now",
        [],
        compression::hooks_resume
    ),
    tool!(
        "compression_hooks_adopt_edit",
        2,
        None,
        "Take over a hook entry that was edited by hand: the ledger records the edit as what Toolport wrote, so it stops reading as edited and undo restores the edit; changes the ledger only, never settings.json; revert=true puts the ledger back",
        [
            p("hook", Ty::Str, true, "rtk or read-once"),
            p("revert", Ty::Bool, false, "Put the ledger back as it was before the edit was adopted")
        ],
        compression::hooks_adopt_edit
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
        "Switch compression off (provider none) and remove the generated shims and registry entry; presets and the pin stay; with teardown=true it also runs `headroom mcp uninstall` and `headroom unwrap claude`, which the plan names and a dry run only lists; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("teardown", Ty::Bool, false, "Also uninstall the engine's MCP entry and unwrap Claude; the engine's own data stays"),
            DRY_ON
        ],
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
        "Re-apply the stored compression policy to the shims, the registry entry and the engine, adopting a legacy mcpm policy when none is stored; mcpm_root names the directory that policy is read from (read only); dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("mcpm_root", Ty::Str, false, "Absolute path of an existing directory to read the legacy mcpm policy from instead of the default location; it is only read"),
            DRY_ON
        ],
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
        servers::list_servers
    ),
    tool!(
        "servers_get",
        1,
        None,
        "Read one server entry without secret values",
        [NAME],
        servers::get
    ),
    tool!(
        "servers_list_profiles",
        1,
        None,
        "List access sets (profiles) and the servers each lists",
        [],
        servers::list_profiles
    ),
    tool!(
        "servers_add_profile_tag",
        2,
        None,
        "List a server in an access set, by the set's name or id; a server that is switched off stays unreachable until it is switched on",
        [NAME, p("profile_tag", Ty::Str, true, "Access set name or id")],
        servers::add_profile_tag
    ),
    tool!(
        "servers_remove_profile_tag",
        2,
        None,
        "Take a server out of an access set, by the set's name or id",
        [NAME, p("profile_tag", Ty::Str, true, "Access set name or id")],
        servers::remove_profile_tag
    ),
    tool!(
        "servers_install",
        3,
        Always,
        "Register a new server",
        [
            NAME,
            p(
                "config",
                Ty::Obj,
                true,
                "Server configuration; env is {NAME: value} for plain settings, never a credential; inheritEnv gives the process the whole shell environment"
            ),
            p("profile_tags", Ty::StrList, false, "Access sets to list it in"),
            p("force", Ty::Bool, false, "Replace an existing entry")
        ],
        servers::install
    ),
    tool!(
        "servers_update_config",
        3,
        Always,
        "Patch a server's configuration, including plain env entries, inheritEnv and maxRequestTimeoutMs (milliseconds, 1 to 86400000; null restores the default)",
        [
            NAME,
            p(
                "patch",
                Ty::Obj,
                true,
                "Fields to change; env is {NAME: value}, null removes a name, a credential is refused"
            )
        ],
        servers::update_config
    ),
    tool!(
        "servers_set_mode",
        3,
        Always,
        "Choose whether a server's tools are listed to clients that get the full tool list (auto) or only found through toolport_search_tools and called by name (search)",
        [NAME, p("mode", Ty::Str, true, "auto or search")],
        servers::set_mode
    ),
    tool!(
        "servers_uninstall",
        4,
        Always,
        "Remove a server and its vault entries",
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
    tool!(
        "servers_restart",
        3,
        UnlessDryRun,
        "Restart one server's process in every running gateway, and in every project folder of a server that runs once per folder, so it runs its current code and its tool list is read again; dry_run is on by default (apply with dry_run=false and confirm=true); if_idle=true restarts only when no call is in flight in any of them and names the busy folders, because a restart ends the calls",
        [
            NAME,
            DRY_ON,
            p("if_idle", Ty::Bool, false, "Restart only when no call is in flight")
        ],
        servers::restart_server
    ),
    tool!(
        "servers_auth",
        3,
        Always,
        "Start a server's sign-in. A stdio server's own auth command runs and its consent URL comes back to open in a browser; a remote server runs upstream's browser sign-in and the call returns when it ends. The server's own callback stores the token; no credential is returned",
        [NAME],
        servers::auth
    ),
    tool!(
        "context_bundle_ls",
        1,
        None,
        "List the context bundles (profiles/<name>.yaml in the skills repository) with what each hides and adds and the folders it is applied in",
        [],
        context::ls
    ),
    tool!(
        "context_bundle_status",
        1,
        None,
        "Report which context bundle is applied in a folder, whether a key it owns changed since, and the keys an undo would leave alone",
        [CWD],
        context::status
    ),
    tool!(
        "context_bundle_apply",
        2,
        None,
        "Apply a context bundle in a folder: skill, plugin, agent and CLAUDE.md hiding goes into the folder's git-ignored .claude/settings.local.json and layers into one managed block of CLAUDE.local.md, with a ledger that undo restores from. Pass dry_run to see the plan and write nothing",
        [
            p("name", Ty::Str, true, "The bundle's name (profiles/<name>.yaml)"),
            p("cwd", Ty::Str, true, "The folder where Claude Code starts; nothing is written above it"),
            DRY
        ],
        context::apply
    ),
    tool!(
        "context_bundle_undo",
        2,
        None,
        "Undo the bundle applied in a folder: put back the keys it replaced and remove its block; a key changed since is left alone and listed in conflicts. Pass dry_run to see the plan and write nothing",
        [CWD, DRY],
        context::undo
    ),
    tool!(
        "context_leave_out",
        2,
        None,
        "Leave one CLAUDE.md or rule file out of a folder's stack, or turn auto memory off there: add writes the file's absolute path into claudeMdExcludes (or autoMemoryEnabled: false) in the folder's git-ignored .claude/settings.local.json, rm puts it back. Ledgered like the other folder controls, so undo removes only what Toolport wrote; the file itself is never changed and a managed policy CLAUDE.md cannot be left out. dry_run is on by default: the call previews and writes only with dry_run=false",
        [
            p("action", Ty::Str, true, "add (leave out) or rm (load again)"),
            p("path", Ty::Str, false, "Absolute path, or ~/..., of the instruction file; leave empty when memory is true"),
            p("memory", Ty::Bool, false, "Act on auto memory instead of a file"),
            p("cwd", Ty::Str, true, "Folder where Claude Code starts; nothing is written above it"),
            DRY_ON
        ],
        context::leave_out
    ),
    tool!(
        "context_loads",
        1,
        None,
        "List the instruction files a Claude Code session in a folder loads (CLAUDE.md files, rules, imports, auto memory index) with an estimated token cost, which are left out and by whom, and the folder's calibration factor from its last measurement. With measured=true the rows the last measured run's InstructionsLoaded log confirms are marked loaded, absent or mismatch. Skills, plugins, agents and tools are not rows. Read only",
        [CWD, p("measured", Ty::Bool, false, "Add the last measurement and mark the rows its log confirms")],
        context::loads
    ),
    tool!(
        "context_compose",
        1,
        None,
        "Show the instruction text Claude Code gets when it starts in a folder, part by part in load order, each with its origin, whether it loads lazily and an estimated token count. Read only",
        [CWD],
        context::compose
    ),
    tool!(
        "context_measure",
        2,
        None,
        "Measure what a folder really loads, in tokens, by asking Claude Code itself: one request per variant (as is, each without entry, the bundle), which spends model tokens and is cached per Claude Code version, model, settings and variant. Without start=true and without a cached answer it only returns the plan and starts nothing. The result separates the estimated instruction rows from the part that is not itemised, gives each variant's delta, verifies the rows against the InstructionsLoaded log and carries the calibration factor",
        [
            CWD,
            p("without", Ty::StrList, false, "Also measure without plugin:<id> or skill:<name> (a skill glob is allowed)"),
            p("bundle", Ty::Str, false, "Also measure what the named bundle would leave"),
            p("model", Ty::Str, false, "Model to ask, haiku by default"),
            p("force", Ty::Bool, false, "Measure again instead of answering from the cache"),
            p("start", Ty::Bool, false, "Start the request; without it a cache miss returns the plan only")
        ],
        context::measure
    ),
    tool!(
        "context_status",
        1,
        None,
        "Everything Toolport knows about a folder's context, read only: the applied bundle, what is left out, which plugins are switched off, the estimate of the instruction rows and the last measurement with its calibration factor. Starts nothing and costs no tokens",
        [CWD],
        context::context_status
    ),
    tool!(
        "tasks_list",
        1,
        None,
        "List the tasks (named, repeatable jobs of steps) with their triggers, last run, next scheduled run and whether a run waits for the person. Disabled drafts only with all",
        [p("all", Ty::Bool, false, "Include disabled tasks")],
        tasks::list
    ),
    tool!(
        "tasks_get",
        1,
        None,
        "Show one task's definition (steps, the secrets it may write, the servers and commands it needs, triggers) and its last five runs. Secret values are never part of it",
        [p("id", Ty::Str, true, "The task id")],
        tasks::get
    ),
    tool!(
        "tasks_history",
        1,
        None,
        "List the runs of a task, newest first, or one run with its step logs (already redacted) when run is given",
        [
            p("id", Ty::Str, false, "Only the runs of this task"),
            p("run", Ty::Str, false, "One run id: returns that run with the log of every step"),
            p("limit", Ty::Int, false, "At most this many runs (default 20)")
        ],
        tasks::history
    ),
    tool!(
        "tasks_missed",
        1,
        None,
        "List the scheduled slots that did not start a run on time, newest first (the last 50 the scheduler keeps): caught up by one run after a quiet period, too old to run, or skipped because a run was live",
        [p("id", Ty::Str, false, "Only the slots of this task")],
        tasks::missed
    ),
    tool!(
        "tasks_run",
        2,
        None,
        "Start a task. The person is asked to approve every run through the approval broker; a denial, a timeout or a missing broker starts nothing. The result is the run record (steps and statuses); a task with a step that needs the person pauses as waiting until they resume it. Pass dry_run to see the steps, the secrets it may write and the servers it needs, without asking and without starting",
        [
            p("id", Ty::Str, true, "The task id"),
            p("dry_run", Ty::Bool, false, "Report the plan only; nothing is asked and nothing starts")
        ],
        tasks::run
    ),
    tool!(
        "tasks_set_enabled",
        2,
        None,
        "Turn a task on or off. Turning one on asks the person to approve it through the approval broker, because its schedule and triggers can then run programs on their own; turning one off asks nothing. Pass dry_run to see the change without making it",
        [
            p("id", Ty::Str, true, "The task id"),
            p("enabled", Ty::Bool, true, "true to turn the task on, false to turn it off"),
            p("dry_run", Ty::Bool, false, "Report the change only; nothing is asked and nothing changes")
        ],
        tasks::set_enabled
    ),
    tool!(
        "tasks_cancel",
        2,
        None,
        "Cancel a running or waiting task run and stop its child process",
        [p("run", Ty::Str, true, "The run id (from tasks_list, tasks_get or tasks_history)")],
        tasks::cancel
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
        uri: "mcpm://inventory/servers",
        name: "inventory-servers",
        mime: "text/plain",
        description: "Servers, transport and source",
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
