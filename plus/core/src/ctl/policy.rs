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

use super::access;
use super::agents;
use super::attention;
use super::auth;
use super::catalog;
use super::client_direct;
use super::client_reach;
use super::context;
use super::flags::{switch, value, Flag, Spec};
use super::launch_profile;
use super::library;
use super::library_select;
use super::mcp;
use super::memory;
use super::plugins;
use super::secret;
use super::server;
use super::server_repo;
use super::setup;
use super::skills;
use super::skills_checks;
use super::skills_repo;
use super::skills_state;
use super::skills_taps;
use super::skills_undo;
use super::sources;
use super::spend;
use super::styles;
use super::sync;
use super::task;
use super::{compression, compression_cfg, compression_hooks};

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

const fn some(name: &'static str) -> Operand {
    Operand {
        name,
        required: true,
        variadic: true,
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

const fn many(name: &'static str) -> Operand {
    Operand {
        name,
        required: false,
        variadic: true,
    }
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

    const fn operand_writes(self) -> Self {
        Self {
            operand_escalates: true,
            ..self
        }
    }

    const fn extra(self, extra: &'static [Flag]) -> Self {
        Self { extra, ..self }
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

    const fn requires(self, flags: &'static [&'static str]) -> Self {
        Self {
            requires: flags,
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
    sub("compression proxy", "up", "Start the local compression proxy"),
    sub("compression proxy", "down", "Stop the local compression proxy"),
    sub("compression proxy", "restart", "Stop and start the local compression proxy"),
    sub("compression hooks", "status", "Show the hook entries, whether they are wrapped, and the counted calls"),
    sub("compression hooks", "apply", "Wrap the existing rtk and read-once entries (--provider, --dry-run)"),
    sub("compression hooks", "undo", "Restore the wrapped entries exactly (--provider, --dry-run)"),
    sub("compression hooks", "pause", "Make the wrapped entries answer nothing until resumed, or for a while and then by themselves (--for, --until)"),
    sub("compression hooks", "resume", "Let the wrapped entries answer again"),
    sub("compression hooks", "adopt-edit", "Take over an entry that was edited by hand, so undo restores the edit (<hook>, --revert)"),
    sub("compression hooks", "verify", "Run the hook commands with a synthetic PreToolUse event and report what came back (<hook>)"),
    sub("compression ledger", "summary", "Summarise the token-savings ledger (--provider, --since)"),
    sub("compression ledger", "record", "Append a savings record to the ledger (--provider, --before, --after, --source, --session)"),
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
    row("server repo status", R).spec(&[&server_repo::STATUS]),
    row("server repo check", W)
        .spec(&[&server_repo::CHECK])
        .skip("fetches from remotes; golden step uses a local remote"),
    row("server repo update", W)
        .dry()
        .spec(&[&server_repo::UPDATE])
        .args(&[req("server")])
        .skip("moves a checkout, runs a build and restarts; its --dry-run is a golden step"),
    row("server repo switch", W)
        .dry()
        .spec(&[&server_repo::SWITCH])
        .args(&[req("server")])
        .skip("switches a checkout's branch, runs a build and restarts; its --dry-run is a golden step"),
    row("server repo undo", W)
        .dry()
        .spec(&[&server_repo::UNDO])
        .args(&[req("server")])
        .skip("moves a checkout back; its --dry-run is a golden step"),
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
    row("client direct add", W)
        .dry()
        .spec(&[&client_direct::WRITE])
        .args(&[req("server")])
        .requires(&["--client"])
        .skip("writes a client file; its --dry-run is a golden step"),
    row("client direct rm", D)
        .dry()
        .spec(&[&client_direct::WRITE])
        .args(&[req("server")])
        .requires(&["--client"])
        .skip("removes a client entry; its --dry-run is a golden step"),
    row("client direct ls", R).spec(&[&client_direct::READ]),
    row("client direct check", R).spec(&[&client_direct::READ]),
    row("client reach", R).spec(&[&client_reach::REACH]),
    row("access-set ls", R),
    row("access-set add", W)
        .dry()
        .spec(&[&access::ADD])
        .args(&[req("name")])
        .skip("creates an access set; its --dry-run is a golden step"),
    row("access-set rm", D)
        .dry()
        .spec(&[&access::RM])
        .args(&[req("set")])
        .skip("deletes an access set; its --dry-run is a golden step"),
    row("access-set default", W)
        .dry()
        .spec(&[&access::DEFAULT])
        .args(&[opt("set")])
        .skip("changes the default access; its --dry-run is a golden step"),
    row("access-set tools", W)
        .reads(&["--tool", "--all", "--none"])
        .dry()
        .spec(&[&access::TOOLS])
        .args(&[opt("set"), opt("server")])
        .smoke(&[]),
    row("client scope", W)
        .dry()
        .spec(&[&access::SCOPE])
        .args(&[req("client"), opt("set")])
        .skip("binds a client to an access set; its --dry-run is a golden step"),
    row("folder route ls", R),
    row("folder route add", W)
        .dry()
        .spec(&[&access::ROUTE_ADD])
        .args(&[req("folder"), req("set")])
        .skip("routes a folder to an access set; its --dry-run is a golden step"),
    row("folder route rm", D)
        .dry()
        .spec(&[&access::ROUTE_RM])
        .args(&[req("folder")])
        .skip("removes a folder route; its --dry-run is a golden step"),
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
    row("memory ls", R).spec(&[&memory::LS]),
    row("memory status", R).spec(&[&memory::STATUS]),
    row("memory set", W)
        .dry()
        .spec(&[&memory::SET])
        .args(&[req("position")])
        .one_of(&[&["--cwd", "--global"]])
        .skip("writes the switch in a repository's or the user's settings; its --dry-run is a golden step"),
    row("memory clear", D)
        .dry()
        .spec(&[&memory::CLEAR])
        .requires(&["--cwd"])
        .skip("moves the memory folder to the trash; its --dry-run is a golden step"),
    row("memory undo", W)
        .dry()
        .spec(&[&memory::UNDO])
        .args(&[opt("id")])
        .skip("moves a cleared memory back; its --dry-run is a golden step"),
    row("memory overview", R).spec(&[&memory::OVERVIEW]),
    row("sessions ls", R).spec(&[&memory::LS]),
    row("sessions clear", D)
        .dry()
        .spec(&[&memory::SESSIONS_CLEAR])
        .requires(&["--cwd"])
        .one_of(&[&["--ids", "--older-than"]])
        .skip("moves sessions to the trash or archive; its --dry-run is a golden step"),
    row("sessions undo", W)
        .dry()
        .spec(&[&memory::SESSIONS_UNDO])
        .args(&[opt("id")])
        .skip("moves cleared sessions back; its --dry-run is a golden step"),
    row("sessions restore", W)
        .dry()
        .spec(&[&memory::RESTORE])
        .args(&[req("id")])
        .skip("moves archived sessions back; its --dry-run is a golden step"),
    row("sessions retention", W)
        .reads(&["--set"])
        .dry()
        .spec(&[&memory::RETENTION])
        .smoke(&[]),
    row("context launch-profile ls", R).spec(&[&launch_profile::LS]),
    row("context launch-profile add", W)
        .dry()
        .spec(&[&launch_profile::ADD])
        .args(&[req("name")])
        .skip("binds a client id in the registry and writes a file; its --dry-run is a golden step"),
    row("context launch-profile rm", D)
        .dry()
        .spec(&[&launch_profile::RM])
        .args(&[req("name")])
        .skip("removes a binding and a file; its --dry-run is a golden step"),
    row("context launch-profile policy", W)
        .dry()
        .spec(&[&launch_profile::POLICY])
        .args(&[req("name")])
        .requires(&["--cwd"])
        .skip("writes a folder's settings; its --dry-run is a golden step"),
    row("plugins ls", W)
        .reads(&["--refresh"])
        .spec(&[&plugins::LS])
        .smoke(&[]),
    row("plugins disable", D)
        .dry()
        .spec(&[&plugins::DISABLE])
        .args(&[req("id")])
        .skip("runs claude plugin disable; its --dry-run is a golden step"),
    row("plugins enable", W)
        .dry()
        .spec(&[&plugins::ENABLE])
        .args(&[req("id")])
        .skip("runs claude plugin enable; its --dry-run is a golden step"),
    row("plugins update", W)
        .dry()
        .needs(&[Needs::Network])
        .spec(&[&plugins::UPDATE])
        .args(&[req("id")])
        .skip("runs claude plugin update, which fetches; its --dry-run is a golden step"),
    row("plugins show", R)
        .spec(&[&plugins::SHOW])
        .args(&[req("id")])
        .smoke(&["nope"]),
    row("plugins config", W)
        .reads(&["--set", "--unset"])
        .dry()
        .spec(&[&plugins::CONFIG])
        .args(&[req("id")])
        .smoke(&["nope"]),
    row("plugins mcp ls", R)
        .spec(&[&plugins::MCP_LS])
        .args(&[req("id")])
        .smoke(&["nope"]),
    row("plugins mcp deny", W)
        .dry()
        .spec(&[&plugins::MCP_DENY])
        .args(&[req("id"), req("server")])
        .requires(&["--cwd"])
        .skip("writes a folder's settings; its --dry-run is a golden step"),
    row("plugins mcp allow", W)
        .dry()
        .spec(&[&plugins::MCP_ALLOW])
        .args(&[req("id"), req("server")])
        .requires(&["--cwd"])
        .skip("writes a folder's settings; its --dry-run is a golden step"),
    row("hooks ls", R).spec(&[&plugins::HOOKS_LS]),
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
        .skip("applies a bundle; its --dry-run is a golden step"),
    row("skills sync", W)
        .dry()
        .spec(&[&skills::SYNC])
        .skip("installs skill files; the plan runs with --dry-run in the skills_sync tests"),
    row("skills undo", W)
        .dry()
        .spec(&[&skills_undo::UNDO])
        .skip("restores the last sync's snapshot; its --dry-run is a golden step"),
    row("skills verify", R).spec(&[&skills_undo::VERIFY]),
    row("skills auto status", R),
    row("skills auto on", W)
        .dry()
        .spec(&[&skills_undo::SWITCH])
        .skip("turns the shipped sync task on; its --dry-run is a golden step"),
    row("skills auto off", W)
        .dry()
        .spec(&[&skills_undo::SWITCH])
        .skip("turns the shipped sync task off; its --dry-run is a golden step"),
    row("skills auto pause", W).skip("sets the pause flag in the data dir; golden step"),
    row("skills auto resume", W).skip("clears the pause flag in the data dir; golden step"),
    row("skills ls", R).spec(&[&skills::LS_SOURCE]),
    row("skills lint", R).spec(&[&skills::LINT]),
    row("attention ls", R).spec(&[&attention::LS]),
    row("attention snooze", W)
        .dry()
        .spec(&[&attention::SNOOZE])
        .skip("writes the hidden list in the data dir; golden step"),
    row("attention skip", W)
        .dry()
        .spec(&[&attention::HIDE])
        .skip("writes the hidden list in the data dir; golden step"),
    row("attention dismiss", W)
        .dry()
        .spec(&[&attention::HIDE])
        .skip("writes the hidden list in the data dir; golden step"),
    row("attention restore", W).skip("writes the hidden list in the data dir; golden step"),
    row("attention undo", W).skip("writes the hidden list in the data dir; golden step"),
    row("attention check", W).skip("fetches from remotes; golden step uses a local remote"),
    row("attention settings", R),
    row("attention notify", W).skip("needs the running app; golden steps cover usage and the answer without an app"),
    row("attention set", W).skip("writes the switches in the data dir; golden step"),
    row("sources ls", R).spec(&[&sources::LS]),
    row("sources root ls", R).spec(&[&sources::ROOT_LS]),
    row("sources root add", W)
        .dry()
        .spec(&[&sources::ROOT_CHANGE])
        .args(&[req("dir")])
        .skip("edits sourceRoots in context.json; its --dry-run is a golden step"),
    row("sources root rm", W)
        .dry()
        .spec(&[&sources::ROOT_CHANGE])
        .args(&[req("dir")])
        .skip("edits sourceRoots in context.json; its --dry-run is a golden step"),
    row("skills files", R)
        .spec(&[&skills_checks::FILES])
        .args(&[req("skill")])
        .smoke(&["demo"]),
    row("skills fix", W)
        .dry()
        .spec(&[&skills_checks::FIX])
        .args(&[req("fix"), many("skill")])
        .skip("sets the execute bit on a hook script or moves a linked file into a skill; its --dry-run and apply are golden steps"),
    row("skills diff", R)
        .spec(&[&skills::DIFF])
        .args(&[opt("skill")]),
    row("skills keep", W)
        .dry()
        .spec(&[&skills::KEEP])
        .args(&[req("skill"), some("file")])
        .skip("records a decision in the data dir; its --dry-run is a golden step"),
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
    row("skills tap add", W)
        .dry()
        .needs(&[Needs::Network])
        .spec(&[&skills_taps::TAP_ADD])
        .args(&[req("repo")])
        .skip("clones a git repository; its --dry-run is a golden step"),
    row("skills tap ls", R).spec(&[&skills_taps::TAP_LS]),
    row("skills tap remove", W)
        .dry()
        .spec(&[&skills_taps::TAP_REMOVE])
        .args(&[req("name")])
        .skip("deletes a clone; its --dry-run is a golden step"),
    row("skills tap update", W)
        .dry()
        .needs(&[Needs::Network])
        .spec(&[&skills_taps::TAP_UPDATE])
        .args(&[opt("name")])
        .skip("pulls the clones; its --dry-run is a golden step"),
    row("skills tap link", W)
        .dry()
        .spec(&[&skills_taps::TAP_LINK])
        .args(&[req("name")])
        .skip("writes the Claude Code settings; its --dry-run is a golden step"),
    row("skills tap unlink", W)
        .dry()
        .spec(&[&skills_taps::TAP_UNLINK])
        .args(&[req("name")])
        .skip("writes the Claude Code settings; its --dry-run is a golden step"),
    row("skills search", R)
        .spec(&[&skills_taps::SEARCH])
        .args(&[some("query")])
        .smoke(&["tapskill"]),
    row("skills install", W)
        .dry()
        .needs(&[Needs::Network])
        .spec(&[&skills_taps::INSTALL])
        .args(&[req("spec")])
        .skip("copies skills out of a tap; its --dry-run is a golden step"),
    row("agents add", W)
        .dry()
        .spec(&[&agents::ADD])
        .args(&[req("name")])
        .skip("creates an agent folder; its --dry-run is a golden step"),
    row("agents ls", R).spec(&[&agents::LS]),
    row("agents lint", R).spec(&[&agents::LINT]),
    row("agents audit", R).spec(&[&agents::AUDIT]),
    row("agents diff", R).spec(&[&agents::DIFF]),
    row("agents status", R).spec(&[&agents::STATUS]),
    row("agents clean", D)
        .dry()
        .spec(&[&agents::CLEAN])
        .skip("removes synced outputs; its --dry-run is a golden step"),
    row("agents uninstall", D)
        .dry()
        .spec(&[&agents::UNINSTALL])
        .args(&[req("name")])
        .skip("deletes an agent; its --dry-run is a golden step"),
    row("agents sync", W)
        .dry()
        .spec(&[&agents::SYNC])
        .skip("writes client outputs; its --dry-run is a golden step"),
    row("styles add", W)
        .dry()
        .spec(&[&styles::ADD])
        .args(&[req("name")])
        .skip("creates a style folder; its --dry-run is a golden step"),
    row("styles ls", R).spec(&[&styles::LS]),
    row("styles lint", R).spec(&[&styles::LINT]),
    row("styles diff", R).spec(&[&styles::DIFF]),
    row("styles status", R).spec(&[&styles::STATUS]),
    row("styles sync", W)
        .dry()
        .spec(&[&styles::SYNC])
        .skip("writes client outputs; its --dry-run is a golden step"),
    row("styles apply", W)
        .dry()
        .spec(&[&styles::APPLY])
        .args(&[req("name")])
        .skip("writes client outputs; its --dry-run is a golden step"),
    row("styles remove", D)
        .dry()
        .spec(&[&styles::REMOVE])
        .skip("removes an applied style; its --dry-run is a golden step"),
    row("styles clean", D)
        .dry()
        .spec(&[&styles::CLEAN])
        .skip("removes synced outputs; its --dry-run is a golden step"),
    row("catalog status", R).spec(&[&catalog::STATUS]),
    row("catalog sync", W)
        .needs(&[Needs::Network, Needs::LongRunning])
        .spec(&[&catalog::SYNC])
        .skip("fetches the whole MCP Registry; unit tests sync from a scripted registry"),
    row("library status", R).spec(&[&library::STATUS]),
    row("library open", R)
        .spec(&[&library::OPEN])
        .args(&[opt("skill")]),
    row("library pull", W)
        .dry()
        .needs(&[Needs::Network])
        .spec(&[&library::PULL])
        .skip("moves the library's branch; its --dry-run is a golden step"),
    row("library connect", W)
        .dry()
        .needs(&[Needs::Network])
        .spec(&[&library::CONNECT])
        .args(&[req("url")])
        .skip("clones a repository; its --dry-run is a golden step"),
    row("library remote add", W)
        .dry()
        .spec(&[&library::REMOTE_ADD])
        .args(&[req("url")])
        .skip("adds a git remote to the library; its --dry-run is a golden step"),
    row("library commit", W)
        .dry()
        .spec(&[&library::COMMIT])
        .requires(&["--message"])
        .skip("commits the library's uncommitted work; its --dry-run is a golden step"),
    row("library push", W)
        .dry()
        .needs(&[Needs::Network])
        .spec(&[&library::CHANGE])
        .skip("commits and pushes the library; its --dry-run is a golden step"),
    row("library select ls", R)
        .spec(&[&library_select::LS])
        .args(&[opt("source")]),
    row("library select set", W)
        .dry()
        .spec(&[&library_select::SET])
        .args(&[req("source")])
        .skip("writes the selection file; its --dry-run is a golden step"),
    row("library select plan", W)
        .needs(&[Needs::Network])
        .spec(&[&library_select::PLAN])
        .args(&[req("source")])
        .skip("fetches the source into a temporary clone; it is a golden step"),
    row("library select apply", W)
        .needs(&[Needs::Network])
        .spec(&[&library_select::APPLY])
        .args(&[req("source")])
        .requires(&["--plan-hash"])
        .skip("generates the slice plugin and writes the Claude Code settings; it is a golden step"),
    row("library select undo", W)
        .spec(&[&library_select::UNDO])
        .args(&[req("source")])
        .skip("restores the settings and the slice plugin; it is a golden step"),
    row("library select check", W)
        .needs(&[Needs::Network])
        .spec(&[&library_select::CHECK])
        .args(&[opt("source")])
        .skip("fetches every selected source; it is a golden step"),
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
    row("compression status", R),
    row("compression presets", W)
        .reads(&["--refresh"])
        .dry()
        .spec(&[&compression_cfg::PRESETS])
        .skip("writes only with --refresh; the refresh preview is a golden step"),
    row("compression enable", W)
        .dry()
        .spec(&[&compression_cfg::ENABLE])
        .skip(
            "writes the policy, shims and the engine's MCP entry; its --dry-run is a golden step",
        ),
    row("compression disable", D)
        .dry()
        .spec(&[&compression_cfg::DISABLE])
        .skip("removes generated files; its --dry-run is a golden step"),
    row("compression set-provider", W)
        .dry()
        .spec(&[&compression_cfg::SET_PROVIDER])
        .args(&[req("provider")])
        .skip("switches the provider; its --dry-run is a golden step"),
    row("compression use", W)
        .dry()
        .spec(&[&compression_cfg::DRY])
        .args(&[req("preset")])
        .skip("switches the preset; its --dry-run is a golden step"),
    row("compression sync", W)
        .dry()
        .spec(&[&compression_cfg::SYNC])
        .skip("re-applies the policy; its --dry-run is a golden step"),
    row("compression pin", W)
        .reads(&["--install", "--refresh"])
        .operand_writes()
        .dry()
        .needs(&[Needs::Network, Needs::LongRunning])
        .spec(&[&compression_cfg::PIN])
        .args(&[opt("version")])
        .smoke(&[]),
    row("compression seal", W)
        .reads(&["--apply"])
        .dry()
        .spec(&[&compression_cfg::SEAL])
        .args(&[opt("preset")])
        .skip("reads the running proxy; its preview is a golden step"),
    row("compression env", R).spec(&[&compression_cfg::ENV]),
    row("compression doctor", R),
    row("compression run", W)
        .plan()
        .needs(&[Needs::TerminalOnly, Needs::LongRunning])
        .terminal()
        .extra(&[switch("--plan"), switch("--force"), value("--cwd")])
        .args(&[many("claude-args")])
        .skip("replaces the process with claude; its --plan is a golden step"),
    row("compression verify", R)
        .needs(&[Needs::LongRunning])
        .spec(&[&compression::VERIFY]),
    row("compression measure", W)
        .reads(&["--out"])
        .spec(&[&compression::MEASURE])
        .smoke(&[]),
    row("compression update", W)
        .reads(&["--accept"])
        .unless("--accept")
        .needs(&[Needs::Network, Needs::LongRunning])
        .spec(&[&compression::UPDATE])
        .skip("moves the engine pin; the preview is a golden step"),
    row("compression proxy up", W)
        .needs(&[Needs::LongRunning])
        .skip("starts a process; the no-engine run is a golden step"),
    row("compression proxy down", W).skip("stops a process; the no-proxy run is a golden step"),
    row("compression proxy restart", W)
        .needs(&[Needs::LongRunning])
        .skip("restarts a process; the no-engine run is a golden step"),
    row("compression hooks status", R),
    row("compression hooks apply", W)
        .dry()
        .spec(&[&compression_hooks::HOOKS])
        .skip("edits the hook entries of Claude settings; plus_ctl_contract runs it in a scratch home"),
    row("compression hooks undo", W)
        .dry()
        .spec(&[&compression_hooks::HOOKS])
        .skip("restores the hook entries of Claude settings; plus_ctl_contract runs it in a scratch home"),
    row("compression hooks pause", W)
        .spec(&[&compression_hooks::PAUSE])
        .skip("records a pause in the hooks ledger; plus_ctl_contract step"),
    row("compression hooks resume", W).skip("ends a pause in the hooks ledger; plus_ctl_contract step"),
    row("compression hooks adopt-edit", W)
        .spec(&[&compression_hooks::ADOPT_EDIT])
        .args(&[req("hook")])
        .skip("records an edited entry in the hooks ledger; plus_ctl_contract step"),
    row("compression hooks verify", W)
        .args(&[opt("hook")])
        .skip("runs the hook commands of Claude settings; plus_ctl_contract runs fixture hooks in a scratch home"),
    row("compression hook", W)
        .needs(&[Needs::TerminalOnly])
        .terminal()
        .args(&[many("command")])
        .skip("run by Claude Code with the hook payload on stdin; unit tests cover the wrapper"),
    row("compression ledger summary", R).spec(&[&compression::LEDGER_SUMMARY]),
    row("compression ledger record", W)
        .spec(&[&compression::LEDGER_RECORD])
        .requires(&["--provider"])
        .skip("appends to the ledger; its golden step runs in a scratch data dir"),
    row("spend status", R).spec(&[&spend::STATUS]),
    row("spend receiver status", R).spec(&[&spend::STATUS]),
    row("spend summary", R).spec(&[&spend::SUMMARY]),
    row("spend export-csv", W)
        .reads(&["--out"])
        .spec(&[&spend::EXPORT])
        .smoke(&[]),
    row("spend prune", D)
        .dry()
        .spec(&[&spend::PRUNE])
        .skip("removes events; plus_ctl_contract runs it on a scratch data dir"),
    row("spend config set", W)
        .spec(&[&spend::CONFIG_SET])
        .skip("writes the Spend settings; plus_ctl_contract runs it on a scratch data dir"),
    row("spend setup plan", R).spec(&[&spend::SETUP_PLAN]),
    row("spend setup apply", W)
        .dry()
        .spec(&[&spend::SETUP_APPLY])
        .skip("writes Claude Code settings; its --dry-run is a golden step and plus_ctl_contract runs it in a scratch home"),
    row("spend setup undo", W)
        .dry()
        .spec(&[&spend::SETUP_UNDO])
        .skip("restores Claude Code settings; plus_ctl_contract runs it in a scratch home"),
    row("task ls", R).spec(&[&task::LS]),
    row("task show", R)
        .spec(&[&task::SHOW])
        .args(&[req("id")])
        .smoke(&["no-such-task"]),
    row("task run", W)
        .dry()
        .needs(&[Needs::LongRunning])
        .spec(&[&task::RUN])
        .args(&[req("id")])
        .skip("starts a run; its --dry-run is a golden step and plus_tasks.rs runs real ones"),
    row("task resume", W)
        .spec(&[&task::RESUME])
        .args(&[req("runId")])
        .skip("marks a run as resumed; plus_ctl_contract round trip"),
    row("task cancel", W)
        .spec(&[&task::CANCEL])
        .args(&[req("runId")])
        .skip("stops a run; plus_ctl_contract round trip"),
    row("task add", W)
        .dry()
        .spec(&[&task::ADD])
        .args(&[req("id")])
        .one_of(&[&["--file", "--from-command"]])
        .skip("writes a task file; plus_ctl_contract round trip"),
    row("task edit", W)
        .dry()
        .spec(&[&task::EDIT])
        .args(&[req("id")])
        .requires(&["--file"])
        .skip("rewrites a task file after a backup; plus_ctl_contract round trip"),
    row("task rm", D)
        .dry()
        .spec(&[&task::RM])
        .args(&[req("id")])
        .skip("removes a task file after a backup; plus_ctl_contract round trip"),
    row("task history", R)
        .spec(&[&task::HISTORY])
        .args(&[opt("id")]),
    row("task missed", R)
        .spec(&[&task::MISSED])
        .args(&[opt("id")]),
    row("task enable", W)
        .dry()
        .spec(&[&task::ENABLE])
        .args(&[req("id")])
        .skip("turns a task on; its --dry-run is a golden step"),
    row("task disable", W)
        .dry()
        .spec(&[&task::DISABLE])
        .args(&[req("id")])
        .skip("turns a task off; its --dry-run is a golden step"),
    row("setup parts", R).spec(&[&setup::PARTS]),
    row("setup export", W)
        .dry()
        .needs(&[Needs::Stdin])
        .spec(&[&setup::EXPORT])
        .skip("writes a zip; its --dry-run and apply are golden steps"),
    row("setup inspect", R)
        .spec(&[&setup::INSPECT])
        .args(&[req("file")])
        .smoke(&["missing.zip"]),
    row("setup restore", W)
        .unless("--apply")
        .needs(&[Needs::Stdin])
        .spec(&[&setup::RESTORE])
        .skip("clones and writes files; its dry run and apply are golden steps"),
    row("setup undo", W)
        .dry()
        .spec(&[&setup::UNDO])
        .skip("reverses a restore run; covered by the setup unit tests"),
    row("setup check", R).spec(&[&setup::CHECK]),
    row("sync status", R).spec(&[&sync::STATUS]),
    row("sync setup", W)
        .dry()
        .needs(&[Needs::Stdin])
        .spec(&[&sync::SETUP])
        .skip("writes the sync settings and the keychain entry; covered by the sync unit tests"),
    row("sync push", W)
        .dry()
        .spec(&[&sync::PUSH])
        .skip("puts a sealed file on the transport; covered by the sync unit tests"),
    row("sync pull", W)
        .unless("--apply")
        .spec(&[&sync::PULL])
        .skip("plans against the transport and applies like a restore; covered by the sync unit tests"),
    row("sync plan", R).spec(&[&sync::PLAN]),
    row("sync repos", R).spec(&[&sync::REPOS]),
    row("sync undo", W)
        .dry()
        .spec(&[&sync::UNDO])
        .skip("reverses a sync pull; covered by the sync unit tests"),
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
    maps("status", R, "status"),
    maps("doctor", R, "doctor"),
    own("flow_diagram", R),
    maps("spend_status", R, "spend status"),
    maps("spend_summary", R, "spend summary"),
    maps("spend_setup_plan", R, "spend setup plan"),
    maps("spend_receiver_status", R, "spend receiver status"),
    maps("spend_export_csv", R, "spend export-csv"),
    maps("spend_config_set", W, "spend config set").on(),
    maps("spend_setup_apply", W, "spend setup apply").on(),
    maps("spend_setup_undo", W, "spend setup undo").on(),
    maps("spend_prune", D, "spend prune").on(),
    maps("compression_status", R, "compression status"),
    maps("compression_measure", R, "compression measure"),
    maps("compression_presets", R, "compression presets"),
    maps("compression_doctor", R, "compression doctor"),
    maps("compression_verify", R, "compression verify"),
    maps("compression_env", R, "compression env"),
    maps("compression_hooks_status", R, "compression hooks status"),
    maps(
        "compression_ledger_summary",
        R,
        "compression ledger summary",
    ),
    maps("compression_pin_status", R, "compression pin"),
    maps("compression_hooks_pause", W, "compression hooks pause"),
    maps("compression_hooks_resume", W, "compression hooks resume"),
    maps(
        "compression_hooks_adopt_edit",
        W,
        "compression hooks adopt-edit",
    ),
    maps("compression_hooks_verify", W, "compression hooks verify"),
    maps("compression_hooks_apply", W, "compression hooks apply").on(),
    maps("compression_hooks_undo", W, "compression hooks undo").on(),
    maps("compression_proxy", W, "compression proxy").param(),
    maps("compression_update", W, "compression update").on(),
    maps("compression_enable", W, "compression enable").on(),
    maps("compression_disable", D, "compression disable").on(),
    maps("compression_set_provider", W, "compression set-provider").on(),
    maps("compression_use", W, "compression use").on(),
    maps("compression_sync", W, "compression sync").on(),
    maps("compression_seal", W, "compression seal").on(),
    maps("servers_list", R, "server ls"),
    maps("servers_get", R, "server info"),
    own("servers_list_profiles", R),
    own("servers_add_profile_tag", W),
    own("servers_remove_profile_tag", W),
    maps("servers_install", W, "server new"),
    maps("servers_install_catalog", W, "server install").on(),
    maps("servers_update_config", W, "server edit"),
    maps("servers_set_mode", W, "server mode"),
    maps("servers_uninstall", D, "server uninstall").on(),
    maps("access_set_ls", R, "access-set ls"),
    maps("access_set_add", W, "access-set add").on(),
    maps("access_set_rm", D, "access-set rm").on(),
    maps("access_set_default", W, "access-set default").on(),
    maps("access_set_tools", W, "access-set tools").on(),
    own("client_discovery_set", W).on(),
    maps("client_scope", W, "client scope").on(),
    maps("folder_route_ls", R, "folder route ls"),
    maps("folder_route_add", W, "folder route add").on(),
    maps("folder_route_rm", D, "folder route rm").on(),
    maps("clients_reach", R, "client reach"),
    own("clients_list", R),
    own("clients_sync", W).on(),
    maps("plugins_ls", R, "plugins ls"),
    maps("plugins_show", R, "plugins show"),
    maps("plugins_config", W, "plugins config").on(),
    maps("plugins_mcp", W, "plugins mcp deny").on(),
    maps("hooks_ls", R, "hooks ls"),
    maps("auth_probe", R, "auth probe"),
    maps("secret_check", R, "secret get"),
    maps("secret_set", W, "secret set").on(),
    maps("secret_rm", D, "secret rm").on(),
    maps("client_direct_ls", R, "client direct ls"),
    maps("client_direct_check", R, "client direct check"),
    maps("client_direct_add", W, "client direct add").on(),
    maps("client_direct_rm", D, "client direct rm").on(),
    own("servers_set_enabled", W).on(),
    maps("servers_restart", W, "server restart").on(),
    maps("servers_repo_status", R, "server repo status"),
    maps("servers_repo_check", W, "server repo check"),
    maps("servers_repo_update", W, "server repo update").on(),
    maps("servers_repo_switch", W, "server repo switch").on(),
    maps("servers_repo_undo", W, "server repo undo").on(),
    maps("servers_auth", W, "auth login"),
    maps("context_bundle_ls", R, "context bundle ls"),
    maps("context_bundle_show", R, "context bundle show"),
    maps("context_bundle_status", R, "context bundle status"),
    maps("context_bundle_config", W, "context bundle config").on(),
    maps("context_bundle_save", W, "context bundle add").on(),
    maps("context_bundle_rm", D, "context bundle rm").on(),
    maps("context_bundle_launch", W, "context bundle launch"),
    maps("context_sync", W, "context sync").on(),
    maps("context_bundle_apply", W, "context bundle apply").param(),
    maps("context_bundle_undo", W, "context bundle undo").param(),
    maps("context_leave_out", W, "context leave-out").on(),
    maps("context_launch_profile_ls", R, "context launch-profile ls"),
    maps(
        "context_launch_profile_add",
        W,
        "context launch-profile add",
    )
    .on(),
    maps("context_launch_profile_rm", D, "context launch-profile rm").on(),
    maps(
        "context_launch_profile_policy",
        W,
        "context launch-profile policy",
    )
    .on(),
    maps("context_loads", R, "context loads"),
    maps("memory_ls", R, "memory ls"),
    maps("memory_status", R, "memory status"),
    maps("memory_set", W, "memory set").on(),
    maps("memory_clear", R, "memory clear"),
    maps("memory_undo", W, "memory undo").on(),
    maps("memory_overview", R, "memory overview"),
    maps("sessions_ls", R, "sessions ls"),
    maps("sessions_clear", R, "sessions clear"),
    maps("sessions_undo", W, "sessions undo").on(),
    maps("sessions_restore", W, "sessions restore").on(),
    maps("sessions_retention", W, "sessions retention").on(),
    maps("context_measure", W, "context measure"),
    maps("context_status", R, "context status"),
    maps("context_compose", R, "context compose"),
    maps("skills_list", R, "skills ls"),
    own("skills_get", R),
    maps("skills_lint", R, "skills lint"),
    maps("skills_files", R, "skills files"),
    maps("attention_list", R, "attention ls"),
    maps("attention_snooze", W, "attention snooze").param(),
    maps("attention_skip", W, "attention skip").param(),
    maps("attention_dismiss", W, "attention dismiss").param(),
    maps("attention_restore", W, "attention restore"),
    maps("attention_undo", W, "attention undo"),
    maps("attention_check", W, "attention check"),
    maps("attention_settings", R, "attention settings"),
    maps("attention_set", W, "attention set"),
    maps("sources_ls", R, "sources ls"),
    maps("sources_root_ls", R, "sources root ls"),
    maps("sources_root_set", W, "sources root add").on(),
    maps("skills_fix", W, "skills fix").on(),
    maps("skills_status", R, "skills status"),
    maps("skills_diff", R, "skills diff"),
    maps("skills_keep", W, "skills keep").on(),
    maps("skills_sync", W, "skills sync").on(),
    maps("skills_undo", W, "skills undo").on(),
    maps("skills_verify", R, "skills verify"),
    maps("skills_auto_status", R, "skills auto status"),
    maps("skills_auto_on", W, "skills auto on").param(),
    maps("skills_auto_off", W, "skills auto off").param(),
    maps("skills_auto_pause", W, "skills auto pause"),
    maps("skills_auto_resume", W, "skills auto resume"),
    own("skills_list_transpilers", R),
    maps("skills_scaffold", W, "skills add").param(),
    maps("skills_init", W, "skills init").on(),
    maps("skills_audit", R, "skills audit"),
    maps("skills_tap_list", R, "skills tap ls"),
    maps("skills_search", R, "skills search"),
    maps("skills_tap_add", W, "skills tap add").on(),
    maps("skills_tap_remove", W, "skills tap remove").on(),
    maps("skills_tap_update", W, "skills tap update").on(),
    maps("skills_tap_link", W, "skills tap link").on(),
    maps("skills_tap_unlink", W, "skills tap unlink").on(),
    maps("skills_install", W, "skills install").on(),
    maps("agents_list", R, "agents ls"),
    own("agents_get", R),
    maps("agents_lint", R, "agents lint"),
    own("agents_list_transpilers", R),
    maps("agents_scaffold", W, "agents add").param(),
    maps("agents_sync", W, "agents sync").on(),
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
    maps("styles_scaffold", W, "styles add").param(),
    maps("styles_sync_tier1", W, "styles sync").on(),
    maps("styles_apply", W, "styles apply").on(),
    maps("styles_diff", R, "styles diff"),
    maps("styles_status", R, "styles status"),
    maps("styles_clean", D, "styles clean").on(),
    own("styles_edit_body", W),
    maps("styles_remove", D, "styles remove").on(),
    maps("catalog_status", R, "catalog status"),
    own("catalog_search", R),
    maps("catalog_sync", W, "catalog sync").on(),
    maps("library_status", R, "library status"),
    maps("library_pull", W, "library pull").on(),
    maps("library_connect", W, "library connect").on(),
    maps("library_remote_add", W, "library remote add").on(),
    maps("library_commit", W, "library commit").on(),
    maps("library_select_ls", R, "library select ls"),
    maps("library_select_plan", W, "library select plan"),
    maps("library_select_set", W, "library select set").on(),
    maps("library_select_apply", W, "library select apply").on(),
    maps("library_select_undo", W, "library select undo"),
    maps("library_select_check", W, "library select check"),
    own("skills_git_push", D).on(),
    own("skills_edit_body", W),
    own("skills_edit_frontmatter", W).param(),
    own("skills_delete", W),
    maps("skills_bundle", W, "skills bundle").on(),
    maps("skills_unbundle", W, "skills unbundle").on(),
    maps("skills_clean", D, "skills clean").on(),
    maps("skills_uninstall", D, "skills uninstall").on(),
    maps("skills_resolve", W, "skills resolve").on(),
    maps("setup_parts", R, "setup parts"),
    maps("setup_export", W, "setup export").on(),
    maps("setup_inspect", R, "setup inspect"),
    maps("setup_check", W, "setup check"),
    maps("setup_restore", W, "setup restore").on(),
    maps("setup_undo", W, "setup undo").on(),
    maps("sync_status", R, "sync status"),
    maps("sync_plan", R, "sync plan"),
    maps("sync_repos", R, "sync repos"),
    maps("tasks_list", R, "task ls"),
    maps("tasks_get", R, "task show"),
    maps("tasks_history", R, "task history"),
    maps("tasks_missed", R, "task missed"),
    maps("tasks_set_enabled", W, "task enable").param(),
    maps("tasks_run", W, "task run").param(),
    maps("tasks_cancel", W, "task cancel"),
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
    let tail = &argv[start + command.path.len()..];
    if id == "compression run" && compression::is_plan_only(tail) {
        return None;
    }
    Some(format!(
        "{id} needs a terminal: show the command and let the user run it there"
    ))
}
