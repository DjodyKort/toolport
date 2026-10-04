//! Safety classification of every `toolportctl` command and every self-management tool (D-058,
//! D-059, MIG-GUI-0). One [`Row`] per leaf of `COMMANDS` plus one per sub-command of the rows
//! that dispatch further ([`SUBS`]); one [`ToolRow`] per self-MCP tool. `toolportctl commands`
//! prints the result, tests fail on a command without a row, a row without a command and a tool
//! whose tier or dry-run default differs from its row.
//!
//! `tier` is the highest tier a row can reach. A row that only reads until a flag escalates it
//! says so with `reads` and lists the flags, so a caller computes the tier of a given argv.

use super::flags::{switch, value, Flag, Spec};
use super::{agents, auth, cc, client, client_direct, client_edit, compression, compression_cfg};
use super::{context, context_manage, council, folders, import, mcp, obs, profile, secret};
use super::{
    hooks, plugins, server, skills, skills_repo, skills_state, skills_taps, sources, styles, sync,
    update,
};
use super::usage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Read,
    Write,
    Destructive,
}

use Tier::{Destructive as D, Read as R, Write as W};

impl Tier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Tier::Read => "read",
            Tier::Write => "write",
            Tier::Destructive => "destructive",
        }
    }

    /// Self-MCP tier 1 reads, 2 and 3 write (3 only behind `confirm`), 4 touches remote state
    /// or removes entries.
    pub const fn of_tool(tier: u8) -> Tier {
        match tier {
            1 => Tier::Read,
            2 | 3 => Tier::Write,
            _ => Tier::Destructive,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    None,
    /// The flag turns the run into a preview that writes nothing.
    Flag(&'static str),
    /// The command previews until the flag is passed.
    UnlessApplied(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    Stdin,
    Browser,
    LongRunning,
    Network,
    TerminalOnly,
}

use Needs::{Browser, LongRunning, Network, Stdin, TerminalOnly};

impl Needs {
    pub const fn as_str(self) -> &'static str {
        match self {
            Needs::Stdin => "stdin",
            Needs::Browser => "browser",
            Needs::LongRunning => "long-running",
            Needs::Network => "network",
            Needs::TerminalOnly => "terminal-only",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Screen,
    Terminal,
}

#[derive(Clone, Copy)]
pub(super) struct Operand {
    pub name: &'static str,
    pub required: bool,
    pub variadic: bool,
}

const fn req(name: &'static str) -> Operand {
    Operand {
        name,
        required: true,
        variadic: false,
    }
}

const fn opt(name: &'static str) -> Operand {
    Operand {
        name,
        required: false,
        variadic: false,
    }
}

const fn many(name: &'static str) -> Operand {
    Operand {
        name,
        required: false,
        variadic: true,
    }
}

const fn some(name: &'static str) -> Operand {
    Operand {
        name,
        required: true,
        variadic: true,
    }
}

#[derive(Clone, Copy)]
pub(super) struct Row {
    pub id: &'static str,
    pub tier: Tier,
    pub reads_by_default: bool,
    pub escalators: &'static [&'static str],
    pub operand_escalates: bool,
    pub preview: Preview,
    pub needs: &'static [Needs],
    pub cost: bool,
    pub surface: Surface,
    pub specs: &'static [&'static Spec],
    pub only: &'static [&'static str],
    pub extra: &'static [Flag],
    pub operands: &'static [Operand],
    pub requires: &'static [&'static str],
    pub one_of: &'static [&'static [&'static str]],
}

const fn row(id: &'static str, tier: Tier) -> Row {
    Row {
        id,
        tier,
        reads_by_default: false,
        escalators: &[],
        operand_escalates: false,
        preview: Preview::None,
        needs: &[],
        cost: false,
        surface: Surface::Screen,
        specs: &[],
        only: &[],
        extra: &[],
        operands: &[],
        requires: &[],
        one_of: &[],
    }
}

impl Row {
    const fn dry(self) -> Self {
        Self {
            preview: Preview::Flag("--dry-run"),
            ..self
        }
    }

    const fn plan(self) -> Self {
        Self {
            preview: Preview::Flag("--plan"),
            ..self
        }
    }

    const fn unless(self, apply: &'static str) -> Self {
        Self {
            preview: Preview::UnlessApplied(apply),
            ..self
        }
    }

    const fn reads(self, escalators: &'static [&'static str]) -> Self {
        Self {
            reads_by_default: true,
            escalators,
            ..self
        }
    }

    /// A positional operand also turns the bare read into a write (`compression pin <version>`).
    const fn operand_writes(self) -> Self {
        Self {
            operand_escalates: true,
            ..self
        }
    }

    const fn needs(self, needs: &'static [Needs]) -> Self {
        Self { needs, ..self }
    }

    /// The command spends model requests (`context measure`).
    const fn costs(self) -> Self {
        Self { cost: true, ..self }
    }

    const fn terminal(self) -> Self {
        Self {
            surface: Surface::Terminal,
            ..self
        }
    }

    const fn spec(self, specs: &'static [&'static Spec]) -> Self {
        Self { specs, ..self }
    }

    const fn only(self, only: &'static [&'static str]) -> Self {
        Self { only, ..self }
    }

    const fn extra(self, extra: &'static [Flag]) -> Self {
        Self { extra, ..self }
    }

    const fn args(self, operands: &'static [Operand]) -> Self {
        Self { operands, ..self }
    }

    const fn requires(self, requires: &'static [&'static str]) -> Self {
        Self { requires, ..self }
    }

    const fn one_of(self, one_of: &'static [&'static [&'static str]]) -> Self {
        Self { one_of, ..self }
    }
}

pub(super) struct Sub {
    pub parent: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
}

const fn sub(parent: &'static str, name: &'static str, summary: &'static str) -> Sub {
    Sub {
        parent,
        name,
        summary,
    }
}

impl Sub {
    pub(super) fn id(&self) -> String {
        format!("{} {}", self.parent, self.name)
    }
}

const PASSPHRASE: &[Flag] = &[switch("--passphrase-stdin"), value("--passphrase-env")];

pub(super) const SUBS: &[Sub] = &[
    sub("sync", "init", "Configure encrypted sync for this machine (--repo, --branch, --machine-id, --reconfigure)"),
    sub("sync", "push", "Encrypt and push the sync bundle (--include-projects, --dry-run)"),
    sub("sync", "pull", "Pull the sync bundle and apply it (--include-projects, --force, --no-resolve, --run-setup, --dry-run)"),
    sub("sync", "diff", "Compare local state with the remote bundle"),
    sub("sync", "status", "Show the sync configuration and last run"),
    sub("sync", "reset", "Remove the remote sync data and the local sync state"),
    sub("sync", "rotate-passphrase", "Re-encrypt the remote bundle under a new passphrase"),
    sub("sync", "add-project", "Add a project folder to the sync set (<path>, --name, --files)"),
    sub("sync", "remove-project", "Remove a project folder from the sync set (<name>)"),
    sub("sync", "git-sync", "Set up or run git-based sync of the data directory (--repo, --branch, --auto, --status, --clear)"),
    sub("sync", "migrate", "Import an mcpm sync bundle directory (<bundleDir>, --include-projects)"),
    sub("council", "install", "Register the council server (--api-key-env)"),
    sub("council", "uninstall", "Remove the council server (--purge-key deletes its API key)"),
    sub("council", "doctor", "Check the council server"),
    sub("council", "tools", "List the council server's tools and resources"),
    sub("mcp", "install", "Register the self-management server (--profile)"),
    sub("mcp", "uninstall", "Remove the self-management server and keep it removed"),
    sub("mcp", "doctor", "Check the self-management server"),
    sub("mcp", "tools", "List the self-management tools and resources"),
    sub("cc", "list", "List Claude Code plugins and their update state (<plugin>, --marketplace)"),
    sub("cc", "update", "Update Claude Code plugins (<plugin>, --marketplace, --dry-run)"),
    sub("compression proxy", "up", "Start the local compression proxy"),
    sub("compression proxy", "down", "Stop the local compression proxy"),
    sub("compression proxy", "restart", "Stop and start the local compression proxy"),
    sub("compression ledger", "summary", "Summarise the token-savings ledger (--provider, --since)"),
    sub("compression ledger", "record", "Append a savings record to the ledger (--provider, --before, --after, --source, --session)"),
];

pub(super) const ROWS: &[Row] = &[
    row("status", R),
    row("doctor", R),
    row("commands", R),
    row("server ls", R),
    row("server search", R)
        .needs(&[Network])
        .spec(&[&server::SEARCH])
        .args(&[many("query")]),
    row("server install", W)
        .needs(&[Network])
        .spec(&[&server::INSTALL])
        .args(&[req("name")]),
    row("server uninstall", D)
        .dry()
        .spec(&[&server::UNINSTALL])
        .args(&[req("server")]),
    row("server info", R).args(&[req("server")]),
    row("server new", W)
        .spec(&[&server::NEW])
        .args(&[req("name")])
        .one_of(&[&["--command", "--url"]]),
    row("server edit", W)
        .spec(&[&server::EDIT])
        .args(&[req("server")]),
    row("inspect", R)
        .needs(&[LongRunning, Network])
        .args(&[req("server")]),
    row("profile inspect", R)
        .needs(&[LongRunning, Network])
        .args(&[opt("profile")]),
    row("profile ls", R).spec(&[&profile::LS]),
    row("profile create", W)
        .dry()
        .spec(&[&profile::CREATE])
        .args(&[req("name")]),
    row("profile edit", W)
        .dry()
        .spec(&[&profile::EDIT])
        .args(&[req("profile")]),
    row("profile rm", D)
        .dry()
        .spec(&[&profile::RM])
        .args(&[req("profile")]),
    row("client ls", R),
    row("client sync", W).dry().spec(&[&client::SYNC]),
    row("client edit", W)
        .dry()
        .spec(&[&client_edit::EDIT])
        .args(&[req("client")]),
    row("client import", W)
        .reads(&["--select", "--all"])
        .dry()
        .spec(&[&client_edit::IMPORT])
        .args(&[req("client")]),
    row("client direct add", W)
        .dry()
        .spec(&[&client_direct::ADD])
        .args(&[req("server")])
        .requires(&["--client"]),
    row("client direct rm", W)
        .dry()
        .spec(&[&client_direct::RM])
        .args(&[req("server")])
        .requires(&["--client"]),
    row("client direct ls", R).spec(&[&client_direct::LS]),
    row("direct run", W)
        .needs(&[TerminalOnly, LongRunning, Stdin])
        .terminal()
        .spec(&[&client_direct::RUN])
        .args(&[req("server")]),
    row("auth statusline", R),
    row("auth hook", R),
    row("auth probe", R).needs(&[Network]).spec(&[&auth::PROBE]),
    row("auth login", W)
        .needs(&[Browser, LongRunning, Network])
        .spec(&[&auth::LOGIN])
        .args(&[req("server")]),
    row("secret set", W)
        .needs(&[Stdin])
        .spec(&[&secret::SET])
        .args(&[req("server"), req("key")]),
    row("secret get", R)
        .spec(&[&secret::GET])
        .args(&[req("server"), req("key")]),
    row("secret rm", D).args(&[req("server"), req("key")]),
    row("context loads", R).spec(&[&context::LOADS]),
    row("context measure", R)
        .needs(&[LongRunning])
        .costs()
        .spec(&[&context::MEASURE]),
    row("context folders", W)
        .reads(&["--enable", "--disable"])
        .spec(&[&folders::FOLDERS]),
    row("context checkpoint-status", R)
        .needs(&[Stdin])
        .spec(&[&context::STATUS]),
    row("context plan", R).spec(&[&context::DEPLOY]),
    row("context apply", W).dry().spec(&[&context::DEPLOY_DRY]),
    row("context sync", W).dry().spec(&[&context::DEPLOY_DRY]),
    row("context init", W).dry().spec(&[&context_manage::INIT]),
    row("context status", R).spec(&[&context_manage::STATUS]),
    row("context client add", W)
        .dry()
        .spec(&[&context_manage::CLIENT_ADD])
        .args(&[req("name")]),
    row("context client list", R).spec(&[&context_manage::CLIENT_LIST]),
    row("context profile add", W)
        .dry()
        .spec(&[&context_manage::PROFILE_ADD])
        .args(&[req("name")]),
    row("context profile list", R).spec(&[&context_manage::PROFILE_LIST]),
    row("context profile remove", D)
        .dry()
        .spec(&[&context_manage::PROFILE_REMOVE])
        .args(&[req("name")]),
    row("context disable", D)
        .dry()
        .spec(&[&context_manage::DISABLE]),
    row("compression status", R),
    row("compression presets", W)
        .reads(&["--refresh"])
        .dry()
        .spec(&[&compression_cfg::PRESETS]),
    row("compression enable", W)
        .dry()
        .spec(&[&compression_cfg::ENABLE]),
    row("compression disable", D)
        .dry()
        .spec(&[&compression_cfg::DISABLE]),
    row("compression set-provider", W)
        .dry()
        .spec(&[&compression_cfg::DRY])
        .args(&[req("provider")]),
    row("compression use", W)
        .dry()
        .spec(&[&compression_cfg::DRY])
        .args(&[req("preset")]),
    row("compression sync", W)
        .dry()
        .spec(&[&compression_cfg::SYNC]),
    row("compression pin", W)
        .reads(&["--install", "--refresh"])
        .operand_writes()
        .dry()
        .needs(&[Network, LongRunning])
        .spec(&[&compression_cfg::PIN])
        .args(&[opt("version")]),
    row("compression seal", W)
        .reads(&["--apply"])
        .dry()
        .spec(&[&compression_cfg::SEAL])
        .args(&[opt("preset")]),
    row("compression env", R).spec(&[&compression_cfg::ENV]),
    row("compression doctor", R),
    row("compression run", W)
        .plan()
        .needs(&[TerminalOnly, LongRunning])
        .terminal()
        .extra(&[switch("--plan"), switch("--force"), value("--cwd")])
        .args(&[many("claude-args")]),
    row("compression verify", R)
        .needs(&[LongRunning])
        .spec(&[&compression::VERIFY]),
    row("compression update", W)
        .reads(&["--accept"])
        .unless("--accept")
        .needs(&[Network, LongRunning])
        .spec(&[&compression::UPDATE]),
    row("import mcpm", W)
        .dry()
        .spec(&[&import::MCPM])
        .args(&[req("config-root")]),
    row("import rename-refs", W)
        .dry()
        .spec(&[&import::RENAME])
        .args(&[req("config-root")])
        .requires(&["--tools", "--paths"]),
    row("sources ls", R).spec(&[&sources::LS]),
    row("plugins ls", R).spec(&[&plugins::LS]),
    row("plugins show", R)
        .spec(&[&plugins::SHOW])
        .args(&[req("id")]),
    row("hooks ls", R).spec(&[&hooks::LS]),
    row("sources root ls", R).spec(&[&sources::ROOT_LS]),
    row("sources root add", W)
        .dry()
        .spec(&[&sources::ROOT_CHANGE])
        .args(&[req("dir")]),
    row("sources root rm", W)
        .dry()
        .spec(&[&sources::ROOT_CHANGE])
        .args(&[req("dir")]),
    row("skills sync", W).dry().spec(&[&skills::SYNC]),
    row("skills ls", R).spec(&[&skills::LS_SOURCE]),
    row("skills lint", R).spec(&[&skills::LINT]),
    row("skills diff", R).spec(&[&skills::LS]),
    row("skills init", W).dry().spec(&[&skills_repo::INIT]),
    row("skills add", W)
        .dry()
        .spec(&[&skills_repo::ADD])
        .args(&[req("name")]),
    row("skills audit", R).spec(&[&skills_repo::AUDIT]),
    row("skills bundle", W).dry().spec(&[&skills_repo::BUNDLE]),
    row("skills unbundle", W)
        .dry()
        .spec(&[&skills_repo::UNBUNDLE])
        .args(&[req("bundle")]),
    row("skills status", R).spec(&[&skills_state::STATUS]),
    row("skills clean", D).dry().spec(&[&skills_state::CLEAN]),
    row("skills uninstall", D)
        .dry()
        .spec(&[&skills_state::UNINSTALL])
        .args(&[req("name")]),
    row("skills resolve", W)
        .dry()
        .spec(&[&skills_state::RESOLVE]),
    row("skills tap add", W)
        .dry()
        .needs(&[Network])
        .spec(&[&skills_taps::TAP_ADD])
        .args(&[req("repo")]),
    row("skills tap ls", R).spec(&[&skills_taps::TAP_LS]),
    row("skills tap remove", W)
        .dry()
        .spec(&[&skills_taps::TAP_REMOVE])
        .args(&[req("name")]),
    row("skills tap update", W)
        .dry()
        .needs(&[Network])
        .spec(&[&skills_taps::TAP_UPDATE])
        .args(&[opt("name")]),
    row("skills search", R)
        .spec(&[&skills_taps::SEARCH])
        .args(&[some("query")]),
    row("skills install", W)
        .dry()
        .needs(&[Network])
        .spec(&[&skills_taps::INSTALL])
        .args(&[req("spec")]),
    row("agents add", W)
        .dry()
        .spec(&[&agents::ADD])
        .args(&[req("name")]),
    row("agents ls", R).spec(&[&agents::LS]),
    row("agents lint", R).spec(&[&agents::LINT]),
    row("agents audit", R).spec(&[&agents::AUDIT]),
    row("agents diff", R).spec(&[&agents::DIFF]),
    row("agents status", R).spec(&[&agents::STATUS]),
    row("agents clean", D).dry().spec(&[&agents::CLEAN]),
    row("agents uninstall", D)
        .dry()
        .spec(&[&agents::UNINSTALL])
        .args(&[req("name")]),
    row("agents sync", W).dry().spec(&[&agents::SYNC]),
    row("styles add", W)
        .dry()
        .spec(&[&styles::ADD])
        .args(&[req("name")]),
    row("styles ls", R).spec(&[&styles::LS]),
    row("styles lint", R).spec(&[&styles::LINT]),
    row("styles diff", R).spec(&[&styles::DIFF]),
    row("styles status", R).spec(&[&styles::STATUS]),
    row("styles sync", W).dry().spec(&[&styles::SYNC]),
    row("styles apply", W)
        .dry()
        .spec(&[&styles::APPLY])
        .args(&[req("name")]),
    row("styles remove", D).dry().spec(&[&styles::REMOVE]),
    row("styles clean", D).dry().spec(&[&styles::CLEAN]),
    row("update", W)
        .reads(&["--apply", "--init"])
        .dry()
        .needs(&[Network])
        .spec(&[&update::SPEC])
        .args(&[opt("server")]),
    row("usage", R).spec(&[&usage::SPEC]),
    row("obs otel enable", W)
        .dry()
        .needs(&[LongRunning])
        .spec(&[&obs::ENABLE]),
    row("obs otel disable", W).dry().spec(&[&obs::DISABLE]),
    row("obs otel status", R).spec(&[&obs::STATUS]),
    row("sync init", W)
        .needs(&[Network, Stdin])
        .extra(PASSPHRASE)
        .requires(&["--repo"])
        .one_of(&[&["--passphrase-stdin", "--passphrase-env"]]),
    row("sync push", D).dry().needs(&[Network]),
    row("sync pull", W).dry().needs(&[Network]),
    row("sync diff", R).needs(&[Network]),
    row("sync status", R),
    row("sync reset", D).needs(&[Network]),
    row("sync rotate-passphrase", D)
        .needs(&[Network, Stdin])
        .extra(PASSPHRASE)
        .one_of(&[&["--passphrase-stdin", "--passphrase-env"]]),
    row("sync add-project", W)
        .args(&[req("path")])
        .requires(&["--name"]),
    row("sync remove-project", W).args(&[req("name")]),
    row("sync git-sync", W).needs(&[Network]),
    row("sync migrate", W)
        .needs(&[Stdin])
        .extra(PASSPHRASE)
        .args(&[req("bundleDir")])
        .one_of(&[&["--passphrase-stdin", "--passphrase-env"]]),
    row("council install", W).spec(&[&council::INSTALL]),
    row("council uninstall", D).spec(&[&council::UNINSTALL]),
    row("council doctor", R),
    row("council tools", R),
    row("mcp install", W).spec(&[&mcp::INSTALL]),
    row("mcp uninstall", D),
    row("mcp doctor", R),
    row("mcp tools", R),
    row("cc list", R)
        .spec(&[&cc::SPEC])
        .only(&["--marketplace"])
        .args(&[opt("plugin")]),
    row("cc update", W)
        .dry()
        .needs(&[Network, LongRunning])
        .spec(&[&cc::SPEC])
        .args(&[opt("plugin")]),
    row("compression proxy up", W).needs(&[LongRunning]),
    row("compression proxy down", W),
    row("compression proxy restart", W).needs(&[LongRunning]),
    row("compression ledger summary", R).spec(&[&compression::LEDGER_SUMMARY]),
    row("compression ledger record", W)
        .spec(&[&compression::LEDGER_RECORD])
        .requires(&["--provider"]),
];

pub(super) fn row_of(id: &str) -> Option<&'static Row> {
    ROWS.iter().find(|r| r.id == id)
}

pub(super) fn sub_spec(id: &str) -> Option<(&'static Spec, &'static [&'static str])> {
    let (parent, name) = id.split_once(' ')?;
    (parent == "sync").then(|| sync::describe(name)).flatten()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolPreview {
    None,
    /// A `dry_run` argument that defaults to false.
    Param,
    /// A `dry_run` argument that defaults to true: the tool only previews until told otherwise.
    DefaultOn,
}

impl ToolPreview {
    pub const fn as_str(self) -> &'static str {
        match self {
            ToolPreview::None => "none",
            ToolPreview::Param => "param",
            ToolPreview::DefaultOn => "default-on",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ToolRow {
    pub name: &'static str,
    pub tier: Tier,
    pub preview: ToolPreview,
    pub command: Option<&'static str>,
}

const fn own(name: &'static str, tier: Tier) -> ToolRow {
    ToolRow {
        name,
        tier,
        preview: ToolPreview::None,
        command: None,
    }
}

const fn maps(name: &'static str, tier: Tier, command: &'static str) -> ToolRow {
    ToolRow {
        name,
        tier,
        preview: ToolPreview::None,
        command: Some(command),
    }
}

impl ToolRow {
    const fn param(self) -> Self {
        Self {
            preview: ToolPreview::Param,
            ..self
        }
    }

    const fn on(self) -> Self {
        Self {
            preview: ToolPreview::DefaultOn,
            ..self
        }
    }
}

pub(super) const TOOL_ROWS: &[ToolRow] = &[
    maps("skills_list", R, "skills ls"),
    maps("sources_ls", R, "sources ls"),
    maps("context_measure", W, "context measure"),
    maps("plugins_ls", R, "plugins ls"),
    maps("plugins_show", R, "plugins show"),
    maps("hooks_ls", R, "hooks ls"),
    own("skills_get", R),
    maps("skills_lint", R, "skills lint"),
    maps("skills_status", R, "skills status"),
    own("skills_list_transpilers", R),
    maps("skills_scaffold", W, "skills add"),
    maps("skills_sync", W, "skills sync").param(),
    maps("skills_tap_list", R, "skills tap ls"),
    maps("skills_search", R, "skills search"),
    maps("skills_tap_add", W, "skills tap add").on(),
    maps("skills_tap_remove", W, "skills tap remove").on(),
    maps("skills_tap_update", W, "skills tap update").on(),
    maps("skills_install", W, "skills install").on(),
    maps("skills_diff", R, "skills diff"),
    maps("skills_audit", R, "skills audit"),
    maps("skills_bundle", W, "skills bundle").on(),
    maps("skills_unbundle", W, "skills unbundle").on(),
    maps("skills_clean", D, "skills clean").on(),
    maps("skills_uninstall", D, "skills uninstall").on(),
    maps("skills_resolve", W, "skills resolve").on(),
    own("skills_edit_body", W),
    own("skills_edit_frontmatter", W),
    own("skills_delete", W),
    own("skills_git_push", D),
    maps("agents_list", R, "agents ls"),
    own("agents_get", R),
    maps("agents_lint", R, "agents lint"),
    own("agents_list_transpilers", R),
    maps("agents_scaffold", W, "agents add"),
    maps("agents_sync", W, "agents sync").param(),
    maps("agents_diff", R, "agents diff"),
    maps("agents_audit", R, "agents audit"),
    maps("agents_status", R, "agents status"),
    maps("agents_clean", D, "agents clean").on(),
    maps("agents_uninstall", D, "agents uninstall").on(),
    own("agents_edit_body", W),
    maps("styles_list", R, "styles ls"),
    own("styles_get", R),
    maps("styles_lint", R, "styles lint"),
    own("styles_active", R),
    own("styles_list_transpilers", R),
    maps("styles_scaffold", W, "styles add"),
    maps("styles_sync_tier1", W, "styles sync").param(),
    maps("styles_apply", W, "styles apply").param(),
    maps("styles_diff", R, "styles diff"),
    maps("styles_status", R, "styles status"),
    maps("styles_clean", D, "styles clean").on(),
    own("styles_edit_body", W),
    maps("styles_remove", D, "styles remove").param(),
    maps("compression_status", R, "compression status"),
    maps("compression_enable", W, "compression enable").on(),
    maps("compression_disable", D, "compression disable").on(),
    maps("compression_set_provider", W, "compression set-provider").on(),
    maps("compression_use", W, "compression use").on(),
    maps("compression_sync", W, "compression sync").on(),
    maps("compression_seal", W, "compression seal").on(),
    maps("servers_list", R, "server ls"),
    maps("servers_get", R, "server info"),
    maps("servers_list_profiles", R, "profile ls"),
    own("servers_detect_source", R),
    own("servers_git_status", R),
    own("servers_check_updates", R),
    own("servers_add_profile_tag", W),
    own("servers_remove_profile_tag", W),
    maps("servers_install", W, "server new"),
    maps("servers_update_config", W, "server edit"),
    own("servers_apply_update", W),
    own("servers_set_mode", W),
    own("servers_fork_sync", W),
    maps("servers_auth", W, "auth login"),
    maps("servers_uninstall", D, "server uninstall"),
    maps("clients_list", R, "client ls"),
    maps("clients_sync", W, "client sync").param(),
    maps("client_direct_ls", R, "client direct ls"),
    maps("client_direct_add", W, "client direct add").on(),
    maps("client_direct_rm", W, "client direct rm").on(),
    maps("sync_push", D, "sync push").param(),
    own("where_am_i", R),
    maps("doctor", R, "doctor"),
    own("flow_diagram", R),
];

/// Commands that must not run through the GUI bridge as they are: they replace themselves with
/// another process and need a terminal (D-062). Returns the reason, `None` when the argv is fine.
pub fn terminal_only(argv: &[String]) -> Option<String> {
    let start = argv.iter().position(|a| !a.starts_with('-'))?;
    let words: Vec<String> = argv[start..]
        .iter()
        .take_while(|a| !a.starts_with('-'))
        .cloned()
        .collect();
    let (command, _) = super::find_command(&words)?;
    let id = command.path.join(" ");
    if row_of(&id)?.surface != Surface::Terminal {
        return None;
    }
    let tail = &argv[start + command.path.len()..];
    if id == "compression run" && compression::is_plan_only(tail) {
        return None;
    }
    Some(format!(
        "{id} needs a terminal: show the command and let the user run it there"
    ))
}
