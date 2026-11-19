use serde_json::{json, Map, Value};
use toolport_plus_api::Host;

use super::{
    access, attention, backend, catalog_index, clients, compression, content, context,
    launch_profile, library, memory, reach, server_repo, servers, setup, skills, sources, spend,
    sync, tasks, ToolError,
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
const ATT_ID: Param = p("id", Ty::Str, true, "The row id from attention_list");
const REPO: Param = p("repo_path", Ty::Str, false, "Skills repository root");
const CLIENTS: Param = p("client_keys", Ty::StrList, false, "Client keys to target");
const CLIENT: Param = p("client", Ty::Str, false, "Limit to one client key");
const GLOBAL: Param = p(
    "global_mode",
    Ty::Bool,
    false,
    "User-level locations, the default; false works inside the repository",
);
const DIRECT_CLIENT: Param = p(
    "client",
    Ty::Str,
    true,
    "Client id, as client_direct_ls shows",
);
const FORCE: Param = p(
    "force",
    Ty::Bool,
    false,
    "Replace or remove an entry Toolport did not write or that was edited since",
);
const PLAN_HASH: Param = p(
    "plan_hash",
    Ty::Str,
    false,
    "Hash of the plan dry_run returned; the write is refused when the plan changed since",
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
        "attention_list",
        1,
        None,
        "List what needs the person right now, computed fresh: missing secrets, sign-ins that need renewing, the secrets vault, tasks waiting for you or failing, library changes held back and updates upstream. Rows are ordered needs-you first, newest first, each with a source and an action; checks lists any area that could not be read or is switched off. Pass hidden to add the rows a snooze, skip or dismissal keeps out. Read only",
        [p("hidden", Ty::Bool, false, "Also list the hidden rows with what hides them")],
        attention::list
    ),
    tool!(
        "attention_snooze",
        2,
        None,
        "Hide a row for a while; it returns by itself when the time passes or the row gets more urgent. A row that needs the person can be snoozed for at most 7 days. Default one day. Pass dry_run to see the change without making it",
        [ATT_ID, p("for", Ty::Str, false, "How long: 1h, 2d, 90m"), p("until", Ty::Str, false, "When it ends, as a timestamp with an offset"), p("dry_run", Ty::Bool, false, "Report without writing")],
        attention::snooze
    ),
    tool!(
        "attention_skip",
        2,
        None,
        "Skip the exact version or commit range a worth-a-look update row is about; the row returns when a newer one appears. Not for rows that need the person",
        [ATT_ID, p("dry_run", Ty::Bool, false, "Report without writing")],
        attention::skip
    ),
    tool!(
        "attention_dismiss",
        2,
        None,
        "Hide a worth-a-look row that carries no version until restored. Not for rows that need the person",
        [ATT_ID, p("dry_run", Ty::Bool, false, "Report without writing")],
        attention::dismiss
    ),
    tool!(
        "attention_restore",
        2,
        None,
        "Show a hidden row again at once",
        [ATT_ID],
        attention::restore
    ),
    tool!(
        "attention_undo",
        2,
        None,
        "Undo the last snooze, skip, dismissal or restore",
        [],
        attention::undo
    ),
    tool!(
        "attention_check",
        2,
        None,
        "Fetch the skill taps, plugin marketplaces, the library and the git checkouts of servers to see new upstream commits, and ask npm, PyPI and Docker Hub for newer releases of the packages servers pin; the one check that uses the network. Applies nothing",
        [],
        attention::check
    ),
    tool!(
        "attention_settings",
        1,
        None,
        "Read which checks are on and which rows may notify. Read only",
        [],
        attention::settings
    ),
    tool!(
        "attention_set",
        2,
        None,
        "Switch a check or a notification on or off: key is notify, check.<name> or notify.<name> with name credentials, updates, access, tasks or library",
        [p("key", Ty::Str, true, "notify, check.<name> or notify.<name>"), p("on", Ty::Bool, true, "true to switch on")],
        attention::set
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
        "Apply a safe lint fix: fix=chmod makes the hook scripts of skills executable; fix=move-link copies the file a SKILL.md link outside the skill folder points at (inside the library, at most 1 MB) into the skill and rewrites the link; dry_run is on by default and lists the copy and the rewritten lines (apply with dry_run=false and confirm=true)",
        [
            p("fix", Ty::Str, true, "The fix to apply: chmod or move-link"),
            REPO,
            p("names", Ty::StrList, false, "Limit to these skills; move-link takes exactly one"),
            p("link", Ty::Str, false, "move-link: the link as written in SKILL.md, e.g. ../shared/guide.md"),
            p("dest", Ty::Str, false, "move-link: the folder of the skill the file is copied to, default references"),
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
        "Transpile skills, agents and output styles to client outputs in one plan with one snapshot; dry_run is on by default (apply with dry_run=false and confirm=true). Removals, new hooks and files that differ wait unless apply_held=true",
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
                "only",
                Ty::StrList,
                false,
                "Sync only these skills and rules, or agent:<name> and style:<name>, and leave the rest of the lock as it is"
            ),
            p(
                "take",
                Ty::StrList,
                false,
                "Replace these held files with the library copy after a backup, each as <skill>/<path in the skill folder>, agent:<name>/<file> or style:<name>/<file>"
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
        "Compare the skills with the lockfile: new, modified, removed and unchanged. With name, show that skill's held files instead (they differ from the library copy or were edited after the last sync): library against installed with a unified diff, binary files flagged; file narrows it to one path of the skill folder",
        [
            REPO,
            p("name", Ty::Str, false, "A skill: show its held files, not the lockfile comparison"),
            p("file", Ty::Str, false, "With name: only this file, relative to the skill folder, whatever its status")
        ],
        skills::diff
    ),
    tool!(
        "skills_keep",
        2,
        UnlessDryRun,
        "Keep files of a skill that differ from the library copy as they are: the next sync stops holding them while their content stays the same; dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            SKILL_NAME,
            p("files", Ty::StrList, true, "Files relative to the skill folder, held by the sync"),
            REPO,
            GLOBAL,
            DRY_ON
        ],
        skills::keep
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
        "Pull one tap or all of them and update the plugins linked to Claude Code; dry_run is on by default",
        [p("name", Ty::Str, false, "Limit to one tap"), DRY_ON],
        skills::tap_update
    ),
    tool!(
        "skills_tap_link",
        2,
        None,
        "Link a plugin marketplace tap to Claude Code at user scope: declare its clone and switch its plugins on; dry_run is on by default",
        [
            p("name", Ty::Str, true, "Tap name"),
            p("plugins", Ty::StrList, false, "Plugins to link, default all of the marketplace"),
            DRY_ON
        ],
        skills::tap_link
    ),
    tool!(
        "skills_tap_unlink",
        2,
        None,
        "Unlink a tap from Claude Code: remove its marketplace declaration and switch its plugins off; dry_run is on by default",
        [p("name", Ty::Str, true, "Tap name"), DRY_ON],
        skills::tap_unlink
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
        "catalog_status",
        1,
        None,
        "Show the local index of the MCP Registry that catalog search answers from: how many servers it holds, when it was last synced, whether it is fresh (synced within 24 hours), stale, syncing, or its last sync failed, and whether an unfinished sync can be continued. Read only; the index is synced in the background and with toolportctl catalog sync",
        [],
        catalog_index::status
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
        "library_connect",
        3,
        UnlessDryRun,
        "Clone a repository as the skills library and point Toolport at it (skills_sync.json in the data folder). Refuses while a library is connected, a target folder that holds files and a url that carries a credential; credentials stay with git and gh. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("url", Ty::Str, true, "https, ssh, git or file url, user@host:path or an absolute path"),
            p("clone_to", Ty::Str, false, "Absolute folder to clone into, default ~/.config/mcpm/skills_repo"),
            DRY_ON
        ],
        library::connect_repository
    ),
    tool!(
        "library_remote_add",
        3,
        UnlessDryRun,
        "Give the skills library without a remote its origin; nothing is fetched or pushed. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("url", Ty::Str, true, "https, ssh, git or file url, user@host:path or an absolute path"),
            DRY_ON
        ],
        library::remote_add
    ),
    tool!(
        "library_commit",
        3,
        UnlessDryRun,
        "Commit the uncommitted work of the skills library with a message after the secret scan a push runs; a finding refuses the commit by rule, file and line, never the matched text. Never pushes. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("message", Ty::Str, true, "The commit message"),
            DRY_ON
        ],
        library::commit
    ),
    tool!(
        "library_select_ls",
        1,
        None,
        "List the sources that have a selection of single skills, agents, commands, hooks and rules, or with source the items of one tap or plugin with their state (on, off, new, removed) and the offer to switch a plugin that is installed whole to its chosen items. Read only",
        [p("source", Ty::Str, false, "A tap, or <tap>/<plugin> for a plugin marketplace tap")],
        library::select_ls
    ),
    tool!(
        "library_select_plan",
        2,
        None,
        "Plan the selection of a tap or plugin: fetch the latest version into a temporary clone and return, per item, what applying would add, drop, update or hold (a hook whose command changed needs a new tick), plus the settings it would write and the plan hash. Only plans: applying and ticking hooks are done with toolportctl",
        [
            p("source", Ty::Str, true, "A tap, or <tap>/<plugin> for a plugin marketplace tap"),
            p("fetch", Ty::Bool, false, "Fetch the latest version (default true); false compares with the local clone"),
            p("switch", Ty::Bool, false, "Include switching off the whole plugin that is installed beside the chosen items"),
            p("deepen", Ty::Bool, false, "Download the whole history to see whether the pinned commit is still in it; costs time and data on a large repository"),
        ],
        library::select_plan
    ),
    tool!(
        "library_select_set",
        3,
        UnlessDryRun,
        "Turn single items of a tap or plugin on or off in the selection; nothing is loaded until library_select_apply runs. A hook cannot be turned on here: it runs a command in every session, so a person ticks it after reading the command (toolportctl library select set --tick-hook). dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("source", Ty::Str, true, "A tap, or <tap>/<plugin> for a plugin marketplace tap"),
            p("on", Ty::StrList, false, "Items to turn on, as library_select_ls names them (a skill, agent, command or rule)"),
            p("off", Ty::StrList, false, "Items to turn off"),
            DRY_ON
        ],
        library::select_set
    ),
    tool!(
        "library_select_apply",
        3,
        UnlessDryRun,
        "Apply the selection of a tap or plugin: generates the slice plugin, enables it in the Claude Code settings and links chosen rules into the Library, as the plan shows. dry_run is on by default and returns the plan with its plan_hash; apply with dry_run=false, that plan_hash and confirm=true, and it is refused when the source or the settings moved since the plan. Never ticks a hook, never replaces a Library folder Toolport did not write (skip names the rules to leave as they are). The result lists skills_sync as the follow-up; it is not run",
        [
            p("source", Ty::Str, true, "A tap, or <tap>/<plugin> for a plugin marketplace tap"),
            PLAN_HASH,
            p("fetch", Ty::Bool, false, "Fetch the latest version (default true); must match the plan the hash came from"),
            p("switch", Ty::Bool, false, "Also switch off the whole plugin that is installed beside the chosen items; must match the plan"),
            p("skip", Ty::StrList, false, "Rules whose Library folder stays as it is"),
            DRY_ON
        ],
        library::select_apply
    ),
    tool!(
        "library_select_undo",
        3,
        Always,
        "Put back what the last library_select_apply of a source changed: the record, the generated slice and the settings entries; refuses when a rule it linked was edited in the Library since",
        [p("source", Ty::Str, true, "A tap, or <tap>/<plugin> for a plugin marketplace tap")],
        library::select_undo
    ),
    tool!(
        "library_select_check",
        2,
        None,
        "Fetch the selected sources and record which have an update plan waiting and whether the pinned commit is still on the remote; applies nothing. deepen downloads the whole history to tell, which costs time and data on a large repository",
        [
            p("source", Ty::Str, false, "Only this source; default every selected one"),
            p("fetch", Ty::Bool, false, "Fetch the latest version (default true); false compares with the local clones"),
            p("deepen", Ty::Bool, false, "Look at the whole history to see whether the pinned commit is still in it")
        ],
        library::select_check
    ),
    tool!(
        "skills_git_push",
        4,
        UnlessDryRun,
        "Commit and push the skills repository to its remote, never forced. dry_run is on by default and returns the plan: the skills audit, the secret scan (gitleaks, else the built-in one), the commits that would go and the undo; a finding refuses the push. Apply with dry_run=false, commit_message and confirm=true",
        [
            p("commit_message", Ty::Str, false, "Commit message for uncommitted work; required to apply"),
            REPO,
            DRY_ON
        ],
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
        "spend_status",
        1,
        None,
        "Show whether model-spend collection is on, the OTLP receiver (address, owner, counters, last error), the last event and the event store against its size and age caps",
        [],
        spend::status
    ),
    tool!(
        "spend_summary",
        1,
        None,
        "Model spend from Claude Code's OpenTelemetry export for a window ending today: totals, by day, by project folder, by model and by task, each figure with its kind (Measured, Counted, Estimate, Not itemised), plus the MCP part. Transcripts fill in sessions without OTel events",
        [
            p("range", Ty::Str, false, "today, 7d (default) or 30d"),
            p("project", Ty::Str, false, "Only this project folder, as the by-project rows name it"),
            p("model", Ty::Str, false, "Only this model, as the by-model rows name it"),
            p("task", Ty::Str, false, "Only this task, as the by-task rows name it (Interactive, Unattributed)")
        ],
        spend::summary
    ),
    tool!(
        "spend_setup_plan",
        1,
        None,
        "Show the five OTel env keys Toolport would write to each Claude Code profile's settings.json, and which already hold another value; writes nothing",
        [
            p("port", Ty::Str, false, "Receiver port (default 4318)")
        ],
        spend::setup_plan
    ),
    tool!(
        "spend_receiver_status",
        1,
        None,
        "Show only the OTLP receiver of model-spend collection: whether collection is on, its state, address, owner, counters and last error, and the last event",
        [],
        spend::receiver_status
    ),
    tool!(
        "spend_export_csv",
        1,
        None,
        "Return the model spend of a window ending today as CSV text (one row per day, project folder and model) with the row count; no file is written",
        [p("range", Ty::Str, false, "today, 7d (default) or 30d")],
        spend::export_csv
    ),
    tool!(
        "spend_config_set",
        3,
        UnlessDryRun,
        "Change the model-spend collection settings: receiver port, how many days of events are kept, the size cap of the event store and whether cost is shown. A new port needs spend_setup_apply afterwards so Claude Code sends to it. dry_run is on by default and reports the settings that would result (apply with dry_run=false and confirm=true)",
        [
            p("port", Ty::Str, false, "Receiver port"),
            p("retention_days", Ty::Int, false, "Days of events to keep"),
            p("max_mb", Ty::Int, false, "Size cap of the event store in MB"),
            p("show_cost", Ty::Bool, false, "Show cost figures"),
            DRY_ON
        ],
        spend::config_set
    ),
    tool!(
        "spend_setup_apply",
        3,
        UnlessDryRun,
        "Turn model-spend collection on: write the five OTel env keys into each Claude Code profile's settings.json and start the receiver. A key that already holds another value is left alone and named in the result; this tool cannot overwrite it. dry_run is on by default and returns the plan (apply with dry_run=false and confirm=true)",
        [
            p("port", Ty::Str, false, "Receiver port (default 4318)"),
            DRY_ON
        ],
        spend::setup_apply
    ),
    tool!(
        "spend_setup_undo",
        3,
        UnlessDryRun,
        "Turn model-spend collection off: remove the OTel env keys Toolport wrote from every Claude Code profile and stop the receiver. The collected events and totals stay; deleting them is not offered here. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [DRY_ON],
        spend::setup_undo
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
            p("since", Ty::Str, false, "Only sessions that started on this date or later, e.g. 2026-11-01"),
            p("format", Ty::Str, false, "json (default) is the report; csv is one row per figure with its population and what it cannot tell, returned as text")
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
        "compression_hooks_verify",
        2,
        None,
        "Run the hook commands of Claude settings with a synthetic PreToolUse event (Bash for rtk, a repeated Read for read-once) in a temporary directory and report pass or fail, latency and what each returned; nothing is counted",
        [p("hook", Ty::Str, false, "rtk or read-once; default both")],
        compression::hooks_verify
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
        "servers_repo_status",
        1,
        None,
        "Where the git checkout each local server runs from stands against the branch it tracks: branch, head, commits ahead and behind, local changes and the time of the last fetch. Reads git state already on disk and fetches nothing; applying an update is not offered here",
        [p(
            "name",
            Ty::Str,
            false,
            "Server id or name; every server that runs from a checkout when omitted"
        )],
        servers::repo_status
    ),
    tool!(
        "servers_repo_check",
        2,
        None,
        "Fetch the tracked remote of every local server that runs from a git checkout, so servers_repo_status and servers_repo_update see what was pushed; changes neither the branch nor the files and records when it looked. The only tool here that reaches the network for a checkout",
        [],
        server_repo::check
    ),
    tool!(
        "servers_repo_update",
        3,
        UnlessDryRun,
        "Fast-forward the tracked branch of a server's checkout to the commit the last check saw, run the build command, and restart the server. HIGH risk: it replaces the code the server runs and runs a command. dry_run is on by default and returns the plan: the commits, the files, the build command and its plan_hash (apply with dry_run=false, that plan_hash and confirm=true; refused when the plan changed since). Only a fast-forward is done: local changes, a diverged or detached branch are refused",
        [
            NAME,
            DRY_ON,
            PLAN_HASH,
            p("build", Ty::Str, false, "Build command to run in the checkout after the update; without it the one agreed earlier runs. A command that carries a secret is refused"),
            p("no_build", Ty::Bool, false, "Skip the build"),
            p("restart_now", Ty::Bool, false, "Restart even while a call is running; calls in flight end. Without it a busy server is not restarted"),
            p("fetch", Ty::Bool, false, "Fetch the tracked remote before planning")
        ],
        server_repo::update
    ),
    tool!(
        "servers_repo_switch",
        3,
        UnlessDryRun,
        "Switch a server's checkout to the branch another machine published for it, build, and restart. HIGH risk, as servers_repo_update. dry_run is on by default and returns the plan with its plan_hash (apply with dry_run=false, that plan_hash and confirm=true). A checkout with local changes or a head that was never pushed is refused",
        [
            NAME,
            DRY_ON,
            PLAN_HASH,
            p("machine", Ty::Str, false, "The machine whose branch to switch to; the only other machine when omitted"),
            p("build", Ty::Str, false, "Build command to run after the switch; without it the one agreed earlier runs"),
            p("no_build", Ty::Bool, false, "Skip the build"),
            p("restart_now", Ty::Bool, false, "Restart even while a call is running; calls in flight end"),
            p("fetch", Ty::Bool, false, "Fetch the tracked remote before planning")
        ],
        server_repo::switch_branch
    ),
    tool!(
        "servers_repo_undo",
        3,
        UnlessDryRun,
        "Take back the one update (or, with of=switch, the one branch switch) of a server's checkout, when nothing else has moved the branch since, and run the same build again. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            NAME,
            DRY_ON,
            p("of", Ty::Str, false, "update (default) or switch"),
            p("no_build", Ty::Bool, false, "Skip the build, which a remembered command that now carries a secret needs"),
            p("restart_now", Ty::Bool, false, "Restart even while a call is running; calls in flight end")
        ],
        server_repo::undo
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
        "Patch a server's configuration, including plain env entries, inheritEnv and maxRequestTimeoutMs (milliseconds, 1 to 86400000; null restores the default) and toolBudgets (seconds per tool name, 0 to 86400 exclusive; null removes one) and serial (true: the gateway sends the server one call at a time)",
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
        "clients_reach",
        1,
        None,
        "Show which access sets, clients and folder mappings reach each server, the servers no access set reaches, and where a folder mapping outranks a client's own set",
        [
            p("name", Ty::Str, false, "Only this server, by id or name"),
            CLIENT
        ],
        reach::run
    ),
    tool!(
        "access_set_ls",
        1,
        None,
        "List the access sets: their servers, whether one is the default access, and the clients and folder routes that use each. Read only",
        [],
        access::set_ls
    ),
    tool!(
        "access_set_add",
        3,
        UnlessDryRun,
        "Create an access set and list servers in it. dry_run is on by default and returns the plan with the command that undoes it (apply with dry_run=false and confirm=true)",
        [
            p("name", Ty::Str, true, "Name of the new set; not starting with @"),
            p("servers", Ty::StrList, false, "Servers to list in it, by id or name"),
            DRY_ON
        ],
        access::set_add
    ),
    tool!(
        "access_set_rm",
        4,
        UnlessDryRun,
        "Delete an access set. A set that clients or folder routes still name is refused unless orphans says what happens to them; the default access cannot be deleted. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("set", Ty::Str, true, "Access set by name or id"),
            p("orphans", Ty::Str, false, "detach unbinds the clients and folders that name it (clients follow the default access); leave deletes the set only and they reach nothing"),
            DRY_ON
        ],
        access::set_rm
    ),
    tool!(
        "access_set_default",
        3,
        UnlessDryRun,
        "Make an access set the default access for clients without their own, or none=true for every enabled server. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("set", Ty::Str, false, "Access set by name or id"),
            p("none", Ty::Bool, false, "Return to every enabled server instead of a set"),
            DRY_ON
        ],
        access::set_default
    ),
    tool!(
        "access_set_tools",
        3,
        UnlessDryRun,
        "Show which tools each access set narrows (no server given), or choose the tools of one server that a set exposes: tools lists the only ones exposed (an empty list exposes none), all=true removes the list so every tool is exposed again. Names are not checked against what the server lists. dry_run is on by default and returns the plan (apply with dry_run=false and confirm=true)",
        [
            p("set", Ty::Str, false, "Access set id or name; every set when listing"),
            p("server", Ty::Str, false, "Server id or name; leave out to list"),
            p("tools", Ty::StrList, false, "The only tool names of the server the set exposes"),
            p("all", Ty::Bool, false, "Expose every tool of the server again"),
            DRY_ON
        ],
        access::set_tools
    ),
    tool!(
        "client_scope",
        3,
        UnlessDryRun,
        "Bind a client to an access set, or none=true so it follows the default access. A folder route still outranks the binding. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("client", Ty::Str, true, "Client id, as clients_reach shows"),
            p("set", Ty::Str, false, "Access set by name or id"),
            p("none", Ty::Bool, false, "Remove the client's own set"),
            DRY_ON
        ],
        access::client_scope
    ),
    tool!(
        "folder_route_ls",
        1,
        None,
        "List the folder routes: each folder's access set and the servers it reaches. A route outranks a client's own set. Read only",
        [],
        access::route_ls
    ),
    tool!(
        "folder_route_add",
        3,
        UnlessDryRun,
        "Route an absolute project folder and everything below it to an access set. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("folder", Ty::Str, true, "Absolute folder path"),
            p("set", Ty::Str, true, "Access set by name or id"),
            DRY_ON
        ],
        access::route_add
    ),
    tool!(
        "folder_route_rm",
        4,
        UnlessDryRun,
        "Remove a folder route. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [p("folder", Ty::Str, true, "The routed folder"), DRY_ON],
        access::route_rm
    ),
    tool!(
        "client_direct_ls",
        1,
        None,
        "List the direct client entries Toolport wrote, with their state: ok, stale, customized, missing, orphan, unrecorded or relocated",
        [CLIENT],
        clients::ls
    ),
    tool!(
        "client_direct_check",
        1,
        None,
        "Check the direct client entries against what Toolport wrote and report whether all are in sync",
        [CLIENT],
        clients::check
    ),
    tool!(
        "client_direct_add",
        3,
        UnlessDryRun,
        "Give one client a direct entry for one server, outside the gateway; stdio servers get a launcher entry that holds no secret, remote servers their address only. dry_run is on by default and returns the plan with its hash (apply with dry_run=false, plan_hash and confirm=true)",
        [NAME, DIRECT_CLIENT, FORCE, DRY_ON, PLAN_HASH],
        clients::add
    ),
    tool!(
        "client_direct_rm",
        4,
        UnlessDryRun,
        "Remove a direct client entry Toolport wrote and its record; dry_run is on by default and returns the plan with its hash (apply with dry_run=false, plan_hash and confirm=true)",
        [NAME, DIRECT_CLIENT, FORCE, DRY_ON, PLAN_HASH],
        clients::rm
    ),
    tool!(
        "servers_uninstall",
        4,
        UnlessDryRun,
        "Remove a server, its client entries and its vault entries; dry_run is on by default and lists what would go (apply with dry_run=false and confirm=true); keep_secrets=true leaves the vault entries",
        [
            NAME,
            p(
                "propagate_to_clients",
                Ty::Bool,
                false,
                "Remove client entries too"
            ),
            p("keep_secrets", Ty::Bool, false, "Leave the server's vault entries in place"),
            DRY_ON
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
        "context_bundle_show",
        1,
        None,
        "Show one context bundle: what it hides and adds, the servers and folders it binds, the yaml, lint issues and the folders it is applied in",
        [p("name", Ty::Str, true, "The bundle's name (profiles/<name>.yaml)")],
        context::bundle_show
    ),
    tool!(
        "context_bundle_config",
        3,
        UnlessDryRun,
        "Read the context bundle settings, or with auto_apply set them: on lets context_sync (and a scheduled sync) apply the bundles bound to folders without anyone asking, off keeps that for a person. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [
            p("auto_apply", Ty::Str, false, "on or off; leave out to read the current setting"),
            DRY_ON
        ],
        context::bundle_config
    ),
    tool!(
        "context_sync",
        3,
        UnlessDryRun,
        "Bring the generated context files up to date: apply the bundles bound to folders when auto_apply is on, and regenerate launch profile files and direct client entries that drifted. dry_run is on by default (apply with dry_run=false and confirm=true)",
        [DRY_ON],
        context::sync
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
        "memory_ls",
        1,
        None,
        "List the auto memory files Claude Code keeps for a folder's repository (name, size, type, last written), or with all=true every repository's memory with the global default. Every worktree and subfolder of a repository share one memory folder. Read only",
        [
            p("cwd", Ty::Str, false, "Any folder of the repository; required unless all is true"),
            p("all", Ty::Bool, false, "List every repository that keeps memory instead of one")
        ],
        memory::ls
    ),
    tool!(
        "memory_status",
        1,
        None,
        "Everything about one repository's memory and history, read only: whether auto memory is on and what decides it (managed settings, the environment, this repository's switch, the global default), where the memory folder is, the instruction files in the order Claude Code reads them with their estimated tokens and whether they are left out, and the sessions with their size and expiry. Sizes are exact; token counts are estimates",
        [CWD],
        memory::status
    ),
    tool!(
        "memory_set",
        2,
        None,
        "Set auto memory for a repository to default, on or off by writing autoMemoryEnabled in the git-ignored .claude/settings.local.json at the repository root, or set the global default in ~/.claude/settings.json with global=true. A managed policy wins and refuses the change. Undo is reported in the result. dry_run is on by default: the call previews and writes only with dry_run=false. Takes effect from the next session",
        [
            p("position", Ty::Str, true, "default (no switch of its own), on or off"),
            p("cwd", Ty::Str, false, "Any folder of the repository; leave empty when global is true"),
            p("global", Ty::Bool, false, "Set the global default instead of one repository"),
            DRY_ON
        ],
        memory::set
    ),
    tool!(
        "memory_clear",
        1,
        None,
        "Plan clearing a repository's auto memory: lists the files, the total, the worktrees that share them and the undo, and changes nothing. An agent does not clear memory; the person does it from Context > This folder, where the folder moves to Toolport's trash for 30 days",
        [CWD, p("also_off", Ty::Bool, false, "Plan turning memory off for the repository as well")],
        memory::clear
    ),
    tool!(
        "memory_undo",
        2,
        None,
        "Put a cleared auto memory back from Toolport's trash (the newest for the folder's repository, or the one called id). dry_run is on by default: the call previews and restores only with dry_run=false. Refuses when the original folder exists again",
        [
            p("id", Ty::Str, false, "Trash entry, as the clear reported it; the newest when empty"),
            p("cwd", Ty::Str, false, "Any folder of the repository when no id is given"),
            DRY_ON
        ],
        memory::undo
    ),
    tool!(
        "memory_overview",
        1,
        None,
        "One row per repository, read only: auto memory (state, who set it, size), instruction files at launch (count, estimated tokens, flags), sessions (count, size, oldest, how many expire this week) and size on disk, with a row for transcript folders that belong to no known repository and the totals. all=true covers every repository; otherwise the folder's repository",
        [
            p("cwd", Ty::Str, false, "Any folder of the repository; required unless all is true"),
            p("all", Ty::Bool, false, "Every repository"),
            p("sort", Ty::Str, false, "name (default), memory, sessions, size or active"),
            p("filter", Ty::Str, false, "memory-off, has-local-file, expiring or large")
        ],
        memory::overview
    ),
    tool!(
        "sessions_ls",
        1,
        None,
        "List a repository's sessions (Claude Code transcripts) with size, age, expiry under the retention period and the folders they come from, or with all=true every repository's totals. Names, sizes and times only: no transcript is opened. Read only",
        [
            p("cwd", Ty::Str, false, "Any folder of the repository; required unless all is true"),
            p("all", Ty::Bool, false, "Totals for every repository")
        ],
        memory::sessions_ls
    ),
    tool!(
        "sessions_clear",
        1,
        None,
        "Plan clearing sessions of a repository by id or by age: lists the sessions and sizes and the undo, and changes nothing. An agent does not clear sessions; the person moves them to Toolport's trash or archive from Context > This folder",
        [
            CWD,
            p("ids", Ty::StrList, false, "Session ids"),
            p("older_than_days", Ty::Int, false, "Sessions not written for more than this many days"),
            p("to", Ty::Str, false, "trash (default) or archive")
        ],
        memory::sessions_clear
    ),
    tool!(
        "sessions_undo",
        2,
        None,
        "Put sessions cleared to Toolport's trash back where they were (the newest for the folder's repository, or the one called id). dry_run is on by default: the call previews and restores only with dry_run=false",
        [
            p("id", Ty::Str, false, "Trash entry, as the clear reported it; the newest when empty"),
            p("cwd", Ty::Str, false, "Any folder of the repository when no id is given"),
            DRY_ON
        ],
        memory::sessions_undo
    ),
    tool!(
        "sessions_retention",
        1,
        None,
        "How long Claude Code keeps sessions (cleanupPeriodDays in the user settings, default 30, at least 1): one value for every project, with no per-repository retention. Read only",
        [],
        memory::sessions_retention
    ),
    tool!(
        "context_launch_profile_ls",
        1,
        None,
        "List the launch profiles: each is a named start of Claude Code with its own client id claude-code-<name> bound to an access set, with its state, the command that starts it and the folder mappings that would outrank it. Read only",
        [
            p("name", Ty::Str, false, "Only this profile"),
            p("cwd", Ty::Str, false, "The folder the launch starts in; narrows the folder warnings to the mappings that cover it")
        ],
        launch_profile::ls
    ),
    tool!(
        "context_launch_profile_add",
        3,
        UnlessDryRun,
        "Add or change a launch profile: bind the client id claude-code-<name> to an access set in the registry, copy Claude Code's discovery override to it and write mcp.json with one gateway entry. dry_run is on by default and returns the plan, including a warning when a folder mapping outranks the profile (apply with dry_run=false and confirm=true)",
        [
            p("name", Ty::Str, true, "Profile name, lower case letters, digits and hyphens"),
            p("set", Ty::Str, false, "Access set by name or id; required for a new profile"),
            p("deny_direct", Ty::Bool, false, "Whether context_launch_profile_policy may render deniedMcpServers for this profile; off by default"),
            DRY_ON
        ],
        launch_profile::add
    ),
    tool!(
        "context_launch_profile_rm",
        4,
        UnlessDryRun,
        "Remove a launch profile: its registry binding, discovery override, mcp.json, record and any rendered deniedMcpServers keys. dry_run is on by default and returns the plan (apply with dry_run=false and confirm=true)",
        [p("name", Ty::Str, true, "Profile name"), DRY_ON],
        launch_profile::rm
    ),
    tool!(
        "context_launch_profile_policy",
        2,
        None,
        "Write deniedMcpServers into a folder's git-ignored .claude/settings.local.json for the Claude Code direct entries a profile's access set leaves out, when the profile's deny_direct switch is on; with the switch off it removes what it wrote. Unverified against a real Claude Code launch. dry_run is on by default",
        [
            p("name", Ty::Str, true, "Profile name"),
            p("cwd", Ty::Str, true, "Folder where Claude Code starts; nothing is written above it"),
            DRY_ON
        ],
        launch_profile::policy
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
        "setup_parts",
        1,
        None,
        "List what a setup file can hold with counts and sizes: repositories (path, remote, branch, unpushed commits, changes, worktrees), servers, access sets, the skills library, tasks, settings, Claude Code user files, memory, sessions and secrets, and the last export. Reads only local git state; sessions and secrets are sized, never read",
        [p("roots", Ty::StrList, false, "Folders to search for repositories")],
        setup::parts
    ),
    tool!(
        "setup_export",
        3,
        UnlessDryRun,
        "Write one setup zip with the readable parts: dry_run is on by default and only lists what would go in (apply with dry_run=false and confirm=true). Secrets and sessions are sealed with a passphrase and are never exported through this tool; a person does that in the app or with toolportctl",
        [
            p("parts", Ty::Str, false, "all, or a comma separated list of repos, servers, access, library, tasks, settings, claudeUser, memory"),
            p("out", Ty::Str, false, "File to write, or a folder to get toolport-setup-<date>.zip in; an existing file is never replaced"),
            p("name", Ty::Str, false, "File name for a folder in out, ending in .zip"),
            p("roots", Ty::StrList, false, "Folders to search for repositories"),
            p("no_worktrees", Ty::Bool, false, "Leave the linked worktrees out of the repository list"),
            p("skip_no_remote", Ty::Bool, false, "Leave out repositories without a remote"),
            p("library_zip", Ty::Bool, false, "Also zip the uncommitted skills and rules of the library"),
            p("without", Ty::StrList, false, "Sub-choices to leave out: claude-instructions, claude-settings, claude-plugins, claude-own, memory-auto, memory-switches, memory-instructions"),
            p("peek", Ty::Str, false, "With dry_run: show what one part would hold"),
            DRY_ON
        ],
        setup::export
    ),
    tool!(
        "setup_inspect",
        1,
        None,
        "Read a setup file and change nothing: its parts with counts, sizes and whether each matches its hash. Sealed parts are named, never opened",
        [p("file", Ty::Str, true, "Path of the setup zip")],
        setup::inspect
    ),
    tool!(
        "setup_check",
        1,
        None,
        "Before a reset: repositories with commits on no remote, no remote, changes, stashes and ignored secret-looking files, the things a person carries, and the age of the last export. Reads local git only",
        [p("roots", Ty::StrList, false, "Folders to search for repositories")],
        setup::check
    ),
    tool!(
        "setup_restore",
        3,
        UnlessDryRun,
        "Plan a restore of a setup file against this Mac and change nothing: each row is add, here, conflict, needs you or skip, with a plan hash. Applying a restore is done by a person in the app or with toolportctl",
        [
            p("file", Ty::Str, true, "Path of the setup zip"),
            p("parts", Ty::Str, false, "Comma separated parts to plan (sealed parts are refused)"),
            p("take", Ty::StrList, false, "Conflict rows to take from the file instead of keeping yours"),
            DRY_ON
        ],
        setup::restore
    ),
    tool!(
        "sync_status",
        1,
        None,
        "Where the sealed setup file is kept, which parts sync, when sync last ran, the rows that wait for a person and whether sync is paused while the keyring is locked. Reads local files only and never returns the passphrase",
        [],
        sync::status
    ),
    tool!(
        "sync_plan",
        1,
        None,
        "Plan the file on the sync transport against this machine and change nothing: each row is add, here, incoming, mine, conflict, needs you or skip, with a plan hash. Reads the remote and uses the passphrase kept in the keychain without asking for it. Pushing and applying are done by a person with toolportctl or in the app",
        [p("take", Ty::StrList, false, "Conflict rows to plan as taken from the other machine")],
        sync::plan
    ),
    tool!(
        "sync_repos",
        1,
        None,
        "The git checkouts of local servers: what this machine publishes through sync and what the other machines published, with whether each is the same, newer, older or diverged from this machine's and whether it is pushed. Reads local files and git only; switching or pulling a branch is done by a person",
        [],
        sync::repos
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
