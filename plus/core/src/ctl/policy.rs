//! Safety classification of every `toolportctl` command and every self-management tool (D-058,
//! D-059). One [`Row`] per leaf of `COMMANDS` plus one per sub-command of the rows that dispatch
//! further ([`SUBS`]); one [`ToolRow`] per self-MCP tool. `toolportctl commands` prints the
//! result, tests fail on a command without a row, a row without a command and a tool whose tier or
//! dry-run default differs from its row.
//!
//! `tier` is the highest tier a row can reach. A row that only reads until a flag escalates it
//! says so with `reads` and lists the flags, so a caller computes the tier of a given argv.
//!
//! Every row also says how the smoke test covers it ([`Smoke`]): a read row runs bare, a row that
//! writes either names an argv that only reads or gives the reason it is not run. The test walks
//! the rows, so a new command is classified by adding its row and nothing else.

use super::auth;
use super::context;
use super::flags::{Flag, Spec};
use super::mcp;
use super::plugins;
use super::secret;
use super::server;
use super::skills;
use super::skills_repo;
use super::skills_state;

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

/// How the smoke test covers a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Smoke {
    /// Runs with these arguments after the command words and must change nothing.
    Read(&'static [&'static str]),
    /// Not run, for this reason.
    Skip(&'static str),
}

#[derive(Clone, Copy)]
pub(super) struct Operand {
    pub name: &'static str,
    pub required: bool,
    pub variadic: bool,
}

const fn opt(name: &'static str) -> Operand {
    Operand {
        name,
        required: false,
        variadic: false,
    }
}

const fn req(name: &'static str) -> Operand {
    Operand {
        name,
        required: true,
        variadic: false,
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
    pub smoke: Smoke,
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
        smoke: match tier {
            Tier::Read => Smoke::Read(&[]),
            _ => Smoke::Skip(""),
        },
    }
}

#[allow(dead_code)]
impl Row {
    const fn dry(self) -> Self {
        Self {
            preview: Preview::Flag("--dry-run"),
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

    const fn needs(self, needs: &'static [Needs]) -> Self {
        Self { needs, ..self }
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

    const fn args(self, operands: &'static [Operand]) -> Self {
        Self { operands, ..self }
    }

    const fn smoke(self, argv: &'static [&'static str]) -> Self {
        Self {
            smoke: Smoke::Read(argv),
            ..self
        }
    }

    const fn one_of(self, groups: &'static [&'static [&'static str]]) -> Self {
        Self {
            one_of: groups,
            ..self
        }
    }

    const fn costs(self) -> Self {
        Self { cost: true, ..self }
    }

    const fn requires(self, flags: &'static [&'static str]) -> Self {
        Self {
            requires: flags,
            ..self
        }
    }

    const fn skip(self, reason: &'static str) -> Self {
        Self {
            smoke: Smoke::Skip(reason),
            ..self
        }
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

pub(super) const SUBS: &[Sub] = &[
    sub("mcp", "install", "Register the self-management server and list it in the access sets clients use (--access-set)"),
    sub("mcp", "uninstall", "Remove the self-management server and keep it removed"),
    sub("mcp", "doctor", "Check the self-management server"),
    sub("mcp", "tools", "List the self-management tools and resources"),
    sub("mcp", "call", "Run one self-management tool in-process (<tool>, --args, --args-stdin)"),
];

pub(super) const ROWS: &[Row] = &[
    row("status", R),
    row("doctor", R),
    row("commands", R),
    row("server ls", R),
    row("server info", R)
        .args(&[req("server")])
        .smoke(&["alpha"]),
    row("server install", W)
        .spec(&[&server::INSTALL])
        .args(&[req("name")])
        .skip("adds a curated catalog server; plus_ctl_contract round trip in a scratch registry"),
    row("server new", W)
        .spec(&[&server::NEW])
        .args(&[req("name")])
        .one_of(&[&["--command", "--url"]])
        .skip("adds a server; plus_ctl_contract round trip in a scratch registry"),
    row("server edit", W)
        .spec(&[&server::EDIT])
        .args(&[req("server")])
        .skip("edits a server; plus_ctl_contract round trip in a scratch registry"),
    row("server mode", W)
        .spec(&[&server::MODE])
        .args(&[req("server"), req("mode")])
        .skip("changes how a server is listed; plus_ctl_contract round trip in a scratch registry"),
    row("server uninstall", D)
        .dry()
        .spec(&[&server::UNINSTALL])
        .args(&[req("server")])
        .skip("removes a server; its --dry-run is a golden step"),
    row("server restart", W)
        .dry()
        .spec(&[&server::RESTART])
        .args(&[req("server")])
        .skip("asks every gateway to restart a server; its --dry-run is a golden step"),
    row("auth statusline", R),
    row("auth hook", R),
    row("auth probe", R).needs(&[Needs::Network]).spec(&[&auth::PROBE]),
    row("auth login", W)
        .needs(&[Needs::Browser, Needs::LongRunning, Needs::Network])
        .spec(&[&auth::LOGIN])
        .args(&[req("server")])
        .skip("starts a sign-in in a browser; plus_ctl_contract runs it against a scripted auth command"),
    row("secret set", W)
        .needs(&[Needs::Stdin])
        .spec(&[&secret::SET])
        .args(&[req("server"), req("key")])
        .skip("stores a value in the vault; the contract case runs it against a scratch vault"),
    row("secret get", R)
        .spec(&[&secret::GET])
        .args(&[req("server"), req("key")])
        .smoke(&["alpha", "API_TOKEN"]),
    row("secret rm", D)
        .args(&[req("server"), req("key")])
        .skip("removes a vault entry; the contract case runs it against a scratch vault"),
    row("context loads", R).spec(&[&context::LOADS]),
    row("context compose", R).spec(&[&context::COMPOSE]),
    row("context status", R).spec(&[&context::CONTEXT_STATUS]),
    row("context measure", R)
        .needs(&[Needs::LongRunning])
        .costs()
        .spec(&[&context::MEASURE]),
    row("context sync", W)
        .dry()
        .spec(&[&context::SYNC])
        .skip("applies bundles to folders; the context goldens cover its --dry-run"),
    row("context bundle ls", R).spec(&[&context::LS]),
    row("context bundle show", R)
        .spec(&[&context::SHOW])
        .args(&[req("name")])
        .smoke(&["nope"]),
    row("context bundle add", W)
        .dry()
        .spec(&[&context::ADD])
        .args(&[req("name")])
        .skip("writes a bundle file; its --dry-run is a golden step"),
    row("context bundle edit", W)
        .dry()
        .spec(&[&context::EDIT])
        .args(&[req("name")])
        .skip("changes a bundle file; its --dry-run is a golden step"),
    row("context bundle rm", D)
        .dry()
        .spec(&[&context::RM])
        .args(&[req("name")])
        .skip("deletes a bundle file; its --dry-run is a golden step"),
    row("context leave-out", W)
        .dry()
        .spec(&[&context::LEAVE_OUT])
        .args(&[req("action"), opt("path")])
        .requires(&["--cwd"])
        .skip("writes a folder's settings; its --dry-run is a golden step"),
    row("plugins off", W)
        .dry()
        .spec(&[&plugins::OFF])
        .args(&[req("id")])
        .requires(&["--cwd"])
        .skip("writes a folder's settings; its --dry-run is a golden step"),
    row("plugins on", W)
        .dry()
        .spec(&[&plugins::ON])
        .args(&[req("id")])
        .requires(&["--cwd"])
        .skip("writes a folder's settings; its --dry-run is a golden step"),
    row("context bundle apply", W)
        .dry()
        .spec(&[&context::APPLY])
        .args(&[req("name")])
        .skip("writes a folder's settings; its --dry-run is a golden step"),
    row("context bundle undo", W)
        .dry()
        .spec(&[&context::UNDO])
        .skip("writes a folder's settings; its --dry-run is a golden step"),
    row("context bundle status", R).spec(&[&context::STATUS]),
    row("context bundle launch", W)
        .spec(&[&context::LAUNCH])
        .args(&[req("name")])
        .skip("writes a settings file under the home directory"),
    row("context bundle config", W)
        .reads(&["--auto-apply"])
        .spec(&[&context::CONFIG])
        .skip("a read without --auto-apply; the switch itself writes context.json"),
    row("context use", W)
        .dry()
        .spec(&[&context::USE])
        .args(&[opt("name")])
        .skip("applies a bundle and routes a folder; its --dry-run is a golden step"),
    row("skills sync", W)
        .dry()
        .spec(&[&skills::SYNC])
        .skip("installs skill files; the plan runs with --dry-run in the skills_sync tests"),
    row("skills ls", R).spec(&[&skills::LS_SOURCE]),
    row("skills lint", R).spec(&[&skills::LINT]),
    row("skills diff", R).spec(&[&skills::DIFF]),
    row("skills init", W)
        .dry()
        .spec(&[&skills_repo::INIT])
        .skip("creates a repository; its --dry-run is a golden step"),
    row("skills add", W)
        .dry()
        .spec(&[&skills_repo::ADD])
        .args(&[req("name")])
        .skip("creates a skill folder; its --dry-run is a golden step"),
    row("skills audit", R).spec(&[&skills_repo::AUDIT]),
    row("skills bundle", W)
        .dry()
        .spec(&[&skills_repo::BUNDLE])
        .skip("writes a zip; its --dry-run and apply are golden steps"),
    row("skills unbundle", W)
        .dry()
        .spec(&[&skills_repo::UNBUNDLE])
        .args(&[req("bundle")])
        .skip("extracts into the repository; its --dry-run is a golden step"),
    row("skills status", R).spec(&[&skills_state::STATUS]),
    row("skills clean", D)
        .dry()
        .spec(&[&skills_state::CLEAN])
        .skip("removes synced outputs; its --dry-run is a golden step"),
    row("skills uninstall", D)
        .dry()
        .spec(&[&skills_state::UNINSTALL])
        .args(&[req("name")])
        .skip("deletes a skill folder; its --dry-run is a golden step"),
    row("skills resolve", W)
        .dry()
        .spec(&[&skills_state::RESOLVE])
        .skip("backs up shadowing files; its --dry-run is a golden step"),
    row("mcp install", W)
        .spec(&[&mcp::INSTALL])
        .skip("registers the server and lists it in access sets; plus_mcp.rs round trip"),
    row("mcp uninstall", D)
        .skip("removes the entry and records the opt-out; plus_mcp.rs round trip"),
    row("mcp doctor", R),
    row("mcp tools", R),
    row("mcp call", W)
        .spec(&[&mcp::CALL])
        .args(&[req("tool")])
        .smoke(&["where_am_i"]),
];

pub(super) fn row_of(id: &str) -> Option<&'static Row> {
    ROWS.iter().find(|r| r.id == id)
}

pub(super) fn sub_spec(_id: &str) -> Option<(&'static Spec, &'static [&'static str])> {
    None
}

/// What a test needs of a row: its id, tier and how the smoke test covers it.
#[derive(Clone, Copy, Debug)]
pub struct RowView {
    pub id: &'static str,
    pub tier: Tier,
    pub smoke: Smoke,
}

pub fn rows() -> Vec<RowView> {
    ROWS.iter()
        .map(|r| RowView {
            id: r.id,
            tier: r.tier,
            smoke: r.smoke,
        })
        .collect()
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

#[allow(dead_code)]
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
    own("where_am_i", R),
    maps("doctor", R, "doctor"),
    own("flow_diagram", R),
    maps("servers_list", R, "server ls"),
    maps("servers_get", R, "server info"),
    own("servers_list_profiles", R),
    own("servers_add_profile_tag", W),
    own("servers_remove_profile_tag", W),
    maps("servers_install", W, "server new"),
    maps("servers_update_config", W, "server edit"),
    maps("servers_set_mode", W, "server mode"),
    maps("servers_uninstall", D, "server uninstall"),
    maps("servers_restart", W, "server restart").on(),
    maps("servers_auth", W, "auth login"),
    maps("context_bundle_ls", R, "context bundle ls"),
    maps("context_bundle_status", R, "context bundle status"),
    maps("context_bundle_apply", W, "context bundle apply").param(),
    maps("context_bundle_undo", W, "context bundle undo").param(),
    maps("context_leave_out", W, "context leave-out").on(),
    maps("context_loads", R, "context loads"),
    maps("context_measure", W, "context measure"),
    maps("context_status", R, "context status"),
    maps("context_compose", R, "context compose"),
    maps("skills_list", R, "skills ls"),
    own("skills_get", R),
    maps("skills_lint", R, "skills lint"),
    maps("skills_status", R, "skills status"),
    maps("skills_diff", R, "skills diff"),
    maps("skills_sync", W, "skills sync").on(),
    own("skills_list_transpilers", R),
    maps("skills_scaffold", W, "skills add"),
    maps("skills_audit", R, "skills audit"),
    maps("skills_bundle", W, "skills bundle").on(),
    maps("skills_unbundle", W, "skills unbundle").on(),
    maps("skills_clean", D, "skills clean").on(),
    maps("skills_uninstall", D, "skills uninstall").on(),
    maps("skills_resolve", W, "skills resolve").on(),
];

#[derive(Clone, Copy, Debug)]
pub struct ToolRowView {
    pub name: &'static str,
    pub tier: Tier,
    pub preview: ToolPreview,
    pub command: Option<&'static str>,
}

pub fn tool_rows() -> Vec<ToolRowView> {
    TOOL_ROWS
        .iter()
        .map(|t| ToolRowView {
            name: t.name,
            tier: t.tier,
            preview: t.preview,
            command: t.command,
        })
        .collect()
}

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
    Some(format!(
        "{id} needs a terminal: show the command and let the user run it there"
    ))
}
