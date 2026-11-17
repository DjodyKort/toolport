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

use super::agents;
use super::auth;
use super::context;
use super::flags::{switch, value, Flag, Spec};
use super::library;
use super::mcp;
use super::plugins;
use super::secret;
use super::server;
use super::skills;
use super::skills_checks;
use super::skills_repo;
use super::skills_state;
use super::skills_taps;
use super::skills_undo;
use super::sources;
use super::styles;
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
        .skip("sets the execute bit on a hook script; its --dry-run and apply are golden steps"),
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
    row("library status", R).spec(&[&library::STATUS]),
    row("library pull", W)
        .dry()
        .needs(&[Needs::Network])
        .spec(&[&library::PULL])
        .skip("moves the library's branch; its --dry-run is a golden step"),
    row("library push", W)
        .dry()
        .needs(&[Needs::Network])
        .spec(&[&library::CHANGE])
        .skip("commits and pushes the library; its --dry-run is a golden step"),
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
    maps("compression_status", R, "compression status"),
    maps("compression_measure", R, "compression measure"),
    maps("compression_hooks_pause", W, "compression hooks pause"),
    maps("compression_hooks_resume", W, "compression hooks resume"),
    maps(
        "compression_hooks_adopt_edit",
        W,
        "compression hooks adopt-edit",
    ),
    maps("compression_hooks_verify", W, "compression hooks verify"),
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
    maps("skills_files", R, "skills files"),
    maps("sources_ls", R, "sources ls"),
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
    maps("skills_scaffold", W, "skills add"),
    maps("skills_audit", R, "skills audit"),
    maps("skills_tap_list", R, "skills tap ls"),
    maps("skills_search", R, "skills search"),
    maps("skills_tap_add", W, "skills tap add").on(),
    maps("skills_tap_remove", W, "skills tap remove").on(),
    maps("skills_tap_update", W, "skills tap update").on(),
    maps("skills_install", W, "skills install").on(),
    maps("agents_list", R, "agents ls"),
    own("agents_get", R),
    maps("agents_lint", R, "agents lint"),
    own("agents_list_transpilers", R),
    maps("agents_scaffold", W, "agents add"),
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
    maps("styles_scaffold", W, "styles add"),
    maps("styles_sync_tier1", W, "styles sync").on(),
    maps("styles_apply", W, "styles apply").on(),
    maps("styles_diff", R, "styles diff"),
    maps("styles_status", R, "styles status"),
    maps("styles_clean", D, "styles clean").on(),
    own("styles_edit_body", W),
    maps("styles_remove", D, "styles remove").on(),
    maps("library_status", R, "library status"),
    maps("library_pull", W, "library pull").on(),
    own("skills_git_push", D),
    own("skills_edit_body", W),
    own("skills_edit_frontmatter", W),
    own("skills_delete", W),
    maps("skills_bundle", W, "skills bundle").on(),
    maps("skills_unbundle", W, "skills unbundle").on(),
    maps("skills_clean", D, "skills clean").on(),
    maps("skills_uninstall", D, "skills uninstall").on(),
    maps("skills_resolve", W, "skills resolve").on(),
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
