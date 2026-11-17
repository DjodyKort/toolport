//! `toolportctl commands`: the registry the GUI builds its forms from. Rows come from `COMMANDS`
//! and the sub-commands of the dispatching rows, the safety fields from [`policy`], the flags
//! from the `Spec` each handler parses with, so none of it can drift from what the command
//! accepts.

use serde_json::{json, Value};

use super::flags::{Flag, Spec};
use super::output::{no_args, CtlError, Output};
use super::policy::{self, Needs, Operand, Preview, Row, Surface, Tier, ToolPreview, SUBS};
use super::COMMANDS;
use crate::selfmcp::TOOLS;
use toolport_plus_api::Host;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ty {
    Bool,
    Str,
    Int,
    Path,
    List,
    Paths,
    Choice(&'static [&'static str]),
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::Bool => "bool",
            Ty::Str => "string",
            Ty::Int => "integer",
            Ty::Path => "path",
            Ty::List => "list",
            Ty::Paths => "paths",
            Ty::Choice(_) => "choice",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Meta {
    pub flag: &'static str,
    pub ty: Ty,
    pub effect: &'static str,
    pub hidden: bool,
    pub sensitive: bool,
    pub repeatable: bool,
}

const fn m(flag: &'static str, ty: Ty, effect: &'static str) -> Meta {
    Meta {
        flag,
        ty,
        effect,
        hidden: false,
        sensitive: false,
        repeatable: false,
    }
}

impl Meta {
    const fn hidden(self) -> Self {
        Self {
            hidden: true,
            ..self
        }
    }

    const fn sensitive(self) -> Self {
        Self {
            sensitive: true,
            ..self
        }
    }

    const fn repeats(self) -> Self {
        Self {
            repeatable: true,
            ..self
        }
    }
}

use Ty::{Bool, Choice, Int, Path, Str};

const ON_OFF: Ty = Ty::Choice(&["on", "off"]);

/// What each flag does, per command row. A flag without an entry fails the policy test.
pub(super) const META: &[(&str, Meta)] = &[
    ("skills sync", m("--repo", Path, "Skills repository (default: the configured one)")),
    ("skills sync", m("--home", Path, "Home folder the outputs go under (default: the user home)")),
    ("skills sync", m("--client", Str, "Client key to sync to (repeatable); default: the clients of the lock, else claude-code").repeats()),
    ("skills sync", m("--project", Bool, "Write inside the repository instead of the user folder")),
    ("skills sync", m("--global", Bool, "Write to the user folder (the default)")),
    ("skills sync", m("--dry-run", Bool, "Report the plan and write nothing")),
    ("skills sync", m("--migrate", Bool, "Back up and replace files that shadow a synced skill")),
    ("skills sync", m("--no-migrate", Bool, "Only report files that shadow a synced skill")),
    ("skills sync", m("--apply-held", Bool, "Also remove files that left the library, install new hooks and replace files that differ (after a backup); without it they wait")),
    ("skills sync", m("--allow-tap-hooks", Bool, "Also install hooks of skills that came from a tap (with --apply-held)")),
    ("skills sync", m("--unattended", Bool, "The run a schedule makes: does nothing while auto-sync is paused, applies new and changed files the lock tracks, and holds removals, new hooks and files that differ; not with --apply-held or --migrate")),
    ("skills undo", m("--repo", Path, "Skills repository (default: the configured one)")),
    ("skills undo", m("--home", Path, "Home folder the outputs went under (default: the user home)")),
    ("skills undo", m("--project", Bool, "Undo the project sync instead of the user one")),
    ("skills undo", m("--global", Bool, "Undo the user sync (the default)")),
    ("skills undo", m("--dry-run", Bool, "Report what would be restored and write nothing")),
    ("skills auto on", m("--dry-run", Bool, "Report the change and write nothing")),
    ("skills auto off", m("--dry-run", Bool, "Report the change and write nothing")),
    ("skills verify", m("--repo", Path, "Skills repository (default: the configured one)")),
    ("skills verify", m("--home", Path, "Home folder the outputs went under (default: the user home)")),
    ("skills verify", m("--client", Str, "Check only this client key (repeatable)").repeats()),
    ("skills verify", m("--project", Bool, "Check the project lock")),
    ("skills verify", m("--global", Bool, "Check the user lock (the default)")),
    ("library status", m("--fetch", Bool, "Contact the remote (a read-only ls-remote, then a fetch) to test the credentials and refresh the counts; the only network call of library status")),
    ("library pull", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("library pull", m("--review", Bool, "After a pull that applied, also show the plan skills sync would carry out; nothing is synced")),
    ("library push", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("skills ls", m("--repo", Path, "Skills repository (default: the configured one)")),
    ("skills ls", m("--home", Path, "Home folder used to judge what Claude Code can read")),
    ("skills ls", m("--source", Str, "Only the items of this source (library, repo:odh, plugin:ecc@ecc)")),
    ("skills lint", m("--repo", Path, "Skills repository (default: the configured one)")),
    ("skills lint", m("--home", Path, "Home folder used to check the emitted outputs")),
    ("skills lint", m("--name", Str, "Lint only this skill (repeatable)").repeats()),
    ("sources ls", m("--source", Str, "Only this source or detector (library, repo:odh, tap:acme-skills)")),
    ("sources ls", m("--kind", Choice(&["skill", "command", "agent", "rule", "memory"]), "Only items of this kind")),
    ("sources ls", m("--items", Bool, "Also list every item (skill, command, agent, rule, memory), not only the sources")),
    ("sources ls", m("--cwd", Path, "Also read the repository around this folder")),
    ("sources ls", m("--root", Path, "Also look for checkouts in this folder (repeatable)").repeats()),
    ("sources ls", m("--deep", Bool, "Double the time and depth budget of every detector")),
    ("sources ls", m("--refresh", Bool, "Ignore the scan cache and read everything again")),
    ("sources ls", m("--budget", Str, "Test seam: <detector>=<milliseconds> time budget for one detector").hidden().repeats()),
    ("sources root add", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("sources root rm", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("skills files", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills files", m("--home", Path, "Home folder the outputs live under")),
    ("skills files", m("--client", Str, "Only this client key (repeatable)").repeats()),
    ("skills files", m("--project", Bool, "Read the project lock")),
    ("skills files", m("--global", Bool, "Read the user lock (the default)")),
    ("skills fix", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills fix", m("--home", Path, "Home folder of the lock")),
    ("skills fix", m("--dry-run", Bool, "Report what would change")),
    ("skills diff", m("--repo", Path, "Skills repository (default: the configured one)")),
    ("skills diff", m("--home", Path, "Home folder of the lock")),
    ("skills diff", m("--project", Bool, "Compare with the project lock")),
    ("skills diff", m("--global", Bool, "Compare with the user lock (the default)")),
    ("skills init", m("--path", Path, "Folder to create the skills repository in")),
    ("skills init", m("--name", Str, "Name of the skills repository")),
    ("skills init", m("--dry-run", Bool, "Report what would be created")),
    ("skills add", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills add", m("--type", Choice(&["skill", "rule"]), "Create a skill or a rule")),
    ("skills add", m("--with-progressive", Bool, "Also create modules, reference, templates and examples folders")),
    ("skills add", m("--dry-run", Bool, "Report what would be created")),
    ("skills audit", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills tap add", m("--name", Str, "Alias for the tap (default: derived from the source)")),
    ("skills tap add", m("--dry-run", Bool, "Report what would be cloned")),
    ("skills tap remove", m("--dry-run", Bool, "Report what would be removed")),
    ("skills tap update", m("--dry-run", Bool, "Report which taps would be pulled")),
    ("skills install", m("--path", Path, "Skills repository to install into (default: the working directory)")),
    ("skills install", m("--no-audit", Bool, "Skip the security audit of the skills")),
    ("skills install", m("--dry-run", Bool, "Report what would be installed")),
    ("skills bundle", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills bundle", m("--output", Path, "Zip to create (default: <repository name>-bundle.zip in the repository)")),
    ("skills bundle", m("--skills", Str, "Only these skills, comma separated")),
    ("skills bundle", m("--dry-run", Bool, "Report what would be packed")),
    ("skills unbundle", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills unbundle", m("--dry-run", Bool, "Report what would be extracted")),
    ("skills status", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills status", m("--home", Path, "Home folder the outputs live under")),
    ("skills status", m("--client", Str, "Only this client key (repeatable)").repeats()),
    ("agents add", m("--path", Path, "Repository folder (default: the configured one)")),
    ("agents add", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("agents ls", m("--path", Path, "Repository folder (default: the configured one)")),
    ("agents lint", m("--path", Path, "Repository folder (default: the configured one)")),
    ("agents audit", m("--path", Path, "Repository folder (default: the configured one)")),
    ("agents diff", m("--path", Path, "Repository folder (default: the configured one)")),
    ("agents status", m("--path", Path, "Repository folder (default: the configured one)")),
    ("agents status", m("--home", Path, "Home directory override (test seam; the GUI never passes it)")),
    ("agents status", m("--strict", Bool, "Exit 1 when synced outputs drifted")),
    ("agents clean", m("--path", Path, "Repository folder (default: the configured one)")),
    ("agents clean", m("--home", Path, "Home directory override (test seam; the GUI never passes it)")),
    ("agents clean", m("--client", Str, "Limit to one client key; repeat for several").repeats()),
    ("agents clean", m("--project", Bool, "Use the project-level location instead of the user level")),
    ("agents clean", m("--global", Bool, "Use the user-level location (the default)")),
    ("agents clean", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("agents uninstall", m("--path", Path, "Repository folder (default: the configured one)")),
    ("agents uninstall", m("--home", Path, "Home directory override (test seam; the GUI never passes it)")),
    ("agents uninstall", m("--project", Bool, "Use the project-level location instead of the user level")),
    ("agents uninstall", m("--global", Bool, "Use the user-level location (the default)")),
    ("agents uninstall", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("agents sync", m("--path", Path, "Repository folder (default: the configured one)")),
    ("agents sync", m("--home", Path, "Home directory override (test seam; the GUI never passes it)")),
    ("agents sync", m("--client", Str, "Limit to one client key; repeat for several").repeats()),
    ("agents sync", m("--project", Bool, "Use the project-level location instead of the user level")),
    ("agents sync", m("--global", Bool, "Use the user-level location (the default)")),
    ("agents sync", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("styles add", m("--path", Path, "Repository folder (default: the configured one)")),
    ("styles add", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("styles ls", m("--path", Path, "Repository folder (default: the configured one)")),
    ("styles lint", m("--path", Path, "Repository folder (default: the configured one)")),
    ("styles diff", m("--path", Path, "Repository folder (default: the configured one)")),
    ("styles status", m("--path", Path, "Repository folder (default: the configured one)")),
    ("styles sync", m("--path", Path, "Repository folder (default: the configured one)")),
    ("styles sync", m("--home", Path, "Home directory override (test seam; the GUI never passes it)")),
    ("styles sync", m("--client", Str, "Limit to one client key; repeat for several").repeats()),
    ("styles sync", m("--project", Bool, "Use the project-level location instead of the user level")),
    ("styles sync", m("--global", Bool, "Use the user-level location (the default)")),
    ("styles sync", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("styles apply", m("--path", Path, "Repository folder (default: the configured one)")),
    ("styles apply", m("--home", Path, "Home directory override (test seam; the GUI never passes it)")),
    ("styles apply", m("--client", Str, "Limit to one client key; repeat for several").repeats()),
    ("styles apply", m("--project", Bool, "Use the project-level location instead of the user level")),
    ("styles apply", m("--global", Bool, "Use the user-level location (the default)")),
    ("styles apply", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("styles remove", m("--path", Path, "Repository folder (default: the configured one)")),
    ("styles remove", m("--home", Path, "Home directory override (test seam; the GUI never passes it)")),
    ("styles remove", m("--client", Str, "Limit to one client key; repeat for several").repeats()),
    ("styles remove", m("--project", Bool, "Use the project-level location instead of the user level")),
    ("styles remove", m("--global", Bool, "Use the user-level location (the default)")),
    ("styles remove", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("styles clean", m("--path", Path, "Repository folder (default: the configured one)")),
    ("styles clean", m("--home", Path, "Home directory override (test seam; the GUI never passes it)")),
    ("styles clean", m("--project", Bool, "Use the project-level location instead of the user level")),
    ("styles clean", m("--global", Bool, "Use the user-level location (the default)")),
    ("styles clean", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("skills status", m("--project", Bool, "Read the project lock")),
    ("skills status", m("--global", Bool, "Read the user lock (the default)")),
    ("skills status", m("--strict", Bool, "Exit 1 when an output is missing or differs from the library")),
    ("skills clean", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills clean", m("--home", Path, "Home folder the outputs live under")),
    ("skills clean", m("--client", Str, "Only this client key")),
    ("skills clean", m("--project", Bool, "Clean the project outputs")),
    ("skills clean", m("--global", Bool, "Clean the user outputs (the default)")),
    ("skills clean", m("--dry-run", Bool, "Report what would be removed")),
    ("skills uninstall", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills uninstall", m("--home", Path, "Home folder the outputs live under")),
    ("skills uninstall", m("--project", Bool, "Work on the project outputs")),
    ("skills uninstall", m("--global", Bool, "Work on the user outputs (the default)")),
    ("skills uninstall", m("--dry-run", Bool, "Report what would be removed")),
    ("skills resolve", m("--path", Path, "Skills repository (default: the configured one)")),
    ("skills resolve", m("--home", Path, "Home folder the outputs live under")),
    ("skills resolve", m("--client", Str, "Only this client key")),
    ("skills resolve", m("--project", Bool, "Work on the project outputs")),
    ("skills resolve", m("--global", Bool, "Work on the user outputs (the default)")),
    ("skills resolve", m("--dry-run", Bool, "Report what would change")),
    ("skills resolve", m("--migrate", Bool, "Back up and replace the shadowing files")),
    ("skills resolve", m("--no-migrate", Bool, "Only report the shadowing files")),
    ("server install", m("--offline", Bool, "Accepted for 1.x scripts: install only reads the curated catalog, which needs no network")),
    ("server new", m("--command", Str, "Program to run; makes the server a local (stdio) one")),
    ("server new", m("--arg", Str, "One argument for the program (repeatable, in order)").repeats()),
    ("server new", m("--env", Str, "A plain KEY=VALUE for the process (repeatable); a credential is refused, use secret set").repeats()),
    ("server new", m("--url", Str, "Endpoint of a remote server (http or https)")),
    ("server new", m("--transport", Choice(&["http", "sse"]), "Transport of a remote server; http unless the URL is an SSE one")),
    ("server new", m("--cwd", Path, "Working directory of the process")),
    ("server new", m("--inherit-env", Choice(&["on", "off"]), "Give the process the whole shell environment; off keeps the fixed set (PATH, HOME, locale, certificates) plus its own entries")),
    ("server edit", m("--name", Str, "New name for the server")),
    ("server edit", m("--command", Str, "New program; makes the server a local (stdio) one")),
    ("server edit", m("--arg", Str, "Replace the arguments; give it once per argument").repeats()),
    ("server edit", m("--env", Str, "Set a plain KEY=VALUE (repeatable); a credential or an existing secret entry is refused").repeats()),
    ("server edit", m("--unset-env", Str, "Remove a plain env entry by name (repeatable)").repeats()),
    ("server edit", m("--url", Str, "New endpoint of a remote server")),
    ("server edit", m("--transport", Choice(&["stdio", "http", "sse"]), "Move the server to this transport")),
    ("server edit", m("--cwd", Path, "New working directory of the process")),
    ("server edit", m("--inherit-env", Choice(&["on", "off"]), "Give the process the whole shell environment, or only the fixed set plus its own entries")),
    ("server edit", m("--declare-client-capabilities", Choice(&["on", "off"]), "Tell this server what the connected client can do (sampling, roots)")),
    ("server edit", m("--forward-instructions", Choice(&["on", "off"]), "Pass this server's instructions on to the client")),
    ("server edit", m("--max-request-timeout-ms", Str, "Longest a call to a local server may run while it keeps reporting progress, in milliseconds (1 to 86400000), or default")),
    ("compression presets", m("--refresh", Bool, "Re-snapshot the presets from the installed engine")),
    ("compression presets", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("compression enable", m("--provider", Str, "Compression provider: headroom, rtk-only, parsec or none")),
    ("compression enable", m("--port", Int, "Proxy port of the active preset")),
    ("compression enable", m("--telemetry", ON_OFF, "Send compression telemetry")),
    ("compression enable", m("--preset", Str, "Preset name")),
    ("compression enable", m("--mode", Choice(&["cache", "token"]), "Compression mode of the preset")),
    ("compression enable", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("compression disable", m("--teardown", Bool, "Also run the engine's own uninstall and unwrap; the engine's data stays")),
    ("compression disable", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("compression set-provider", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("compression use", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("compression sync", m("--mcpm-root", Path, "mcpm configuration root to adopt the compression policy from; it is only read")),
    ("compression sync", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("compression pin", m("--install", Bool, "Install the pinned engine build with uv")),
    ("compression pin", m("--refresh", Bool, "Re-snapshot the presets from the installed engine")),
    ("compression pin", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("compression seal", m("--apply", Bool, "Write the declared posture to the policy")),
    ("compression seal", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("compression env", m("--cwd", Path, "Directory whose policy applies")),
    ("compression run", m("--plan", Bool, "Print what would run instead of launching")),
    ("compression run", m("--force", Bool, "Launch even when the installed engine does not match the pin")),
    ("compression run", m("--cwd", Path, "Directory whose policy applies")),
    ("compression measure", m("--format", Choice(&["json", "csv"]), "json is the report itself; csv is one row per figure with its population and what it cannot tell")),
    ("compression measure", m("--out", Path, "Write the report to this file instead of printing it; the answer names the file")),
    ("compression measure", m("--min-turns", Int, "Sessions with fewer turns are left out of the billed outcome and the rework check")),
    ("compression measure", m("--since", Str, "Only sessions that started on this date or later (2026-11-01)")),
    ("compression measure", m("--transcripts", Path, "Directory of Claude Code transcripts to read")),
    ("compression verify", m("--by-pin", Bool, "Group the measured sessions by engine pin")),
    ("compression verify", m("--limit", Int, "Newest transcripts to read")),
    ("compression verify", m("--min-turns", Int, "Sessions with fewer turns are left out")),
    ("compression verify", m("--transcripts", Path, "Directory of Claude Code transcripts to measure")),
    ("compression update", m("--latest", Bool, "Move the pin to the newest engine release")),
    ("compression update", m("--accept", Bool, "Apply the pin change")),
    ("compression update", m("--to", Str, "Engine version to pin")),
    ("compression ledger summary", m("--provider", Str, "Compression provider: headroom, rtk-only, parsec or none")),
    ("compression ledger summary", m("--since", Str, "Only records from this date on")),
    ("compression ledger record", m("--provider", Str, "Compression provider: headroom, rtk-only, parsec or none")),
    ("compression ledger record", m("--before", Int, "Tokens before compression")),
    ("compression ledger record", m("--after", Int, "Tokens after compression")),
    ("compression ledger record", m("--source", Str, "Where the record came from")),
    ("compression ledger record", m("--session", Str, "Session the record belongs to")),
    ("compression enable", m("--accept-egress", Str, "Name the provider to accept that its closed engine sends code off this machine; asked for parsec only")),
    ("compression set-provider", m("--accept-egress", Str, "Name the provider to accept that its closed engine sends code off this machine; asked for parsec only")),
    ("compression hooks apply", m("--dry-run", Bool, "Show what would be wrapped and change nothing")),
    ("compression hooks apply", m("--provider", Choice(&["rtk", "read-once"]), "Wrap only this provider's entry")),
    ("compression hooks undo", m("--dry-run", Bool, "Show what would be restored and change nothing")),
    ("compression hooks undo", m("--provider", Choice(&["rtk", "read-once"]), "Restore only this provider's entry")),
    ("compression hooks pause", m("--for", Str, "How long the pause lasts, e.g. 1d, 6h, 90m or 1d12h; at most 30 days")),
    ("compression hooks pause", m("--until", Str, "When the pause ends: an RFC 3339 time, 2026-11-18T09:12 (UTC) or a date; at most 30 days ahead")),
    ("compression hooks adopt-edit", m("--revert", Bool, "Put the ledger back as it was before the edit was adopted")),
    ("server uninstall", m("--dry-run", Bool, "Show what would be removed and change nothing")),
    ("server uninstall", m("--keep-clients", Bool, "Leave client entries alone; none exist per server on the gateway, direct entries are handled with them")),
    ("server uninstall", m("--keep-secrets", Bool, "Leave the server's vault entries in place")),
    ("task ls", m("--all", Bool, "Include disabled tasks (drafts made from a command file)")),
    ("task add", m("--file", Path, "A task definition (JSON), or - to read it from stdin; the id inside must equal <id>")),
    ("task edit", m("--file", Path, "The complete new task definition (JSON), or - to read it from stdin; the id inside must equal <id>")),
    ("task run", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("task run", m("--wait", Bool, "Stay until the run ends or waits for you; step lines go to stderr")),
    ("task run", m("--yes", Bool, "Accepted for mcpm compatibility; has no effect")),
    ("task add", m("--from-command", Path, "A Claude command file to turn into a disabled draft (a prompt step plus needs-you markers)")),
    ("task add", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("task edit", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("task rm", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("task history", m("--run", Str, "Show this one run with its step logs")),
    ("task history", m("--limit", Int, "Show at most this many results")),
    ("task enable", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("task disable", m("--dry-run", Bool, "Preview the change and write nothing")),
    ("server restart", m("--dry-run", Bool, "Say what would be restarted and ask no gateway")),
    ("secret set", m("--value-env", Str, "Name of an environment variable holding the value (the GUI sends it on stdin)").hidden().sensitive()),
    ("secret get", m("--reveal", Bool, "Print the secret value").sensitive()),
    ("auth probe", m("--server", Str, "Server id")),
    ("auth probe", m("--force", Bool, "Probe every server, not only the ones that are due")),
    ("auth login", m("--no-open", Bool, "Do not open the browser; print the sign-in link")),
    ("context bundle add", m("--servers", Str, "Access set paired with the bundle, by name or id (default: the set with the same name)")),
    ("context bundle edit", m("--servers", Str, "Access set paired with the bundle (empty removes the pairing)")),
    ("context bundle rm", m("--force", Bool, "Delete even while the bundle is applied in a folder")),
    ("context leave-out", m("--cwd", Path, "Folder whose .claude/settings.local.json gets the entry (required)")),
    ("context leave-out", m("--memory", Bool, "Act on auto memory (autoMemoryEnabled) instead of a file")),
    ("plugins off", m("--cwd", Path, "Folder whose .claude/settings.local.json gets enabledPlugins.<id> = false (required)")),
    ("plugins on", m("--cwd", Path, "Folder whose .claude/settings.local.json loses the entry `plugins off` wrote (required)")),
    ("plugins off", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("plugins on", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("memory ls", m("--cwd", Path, "Any folder of the repository (default: the current folder)")),
    ("memory ls", m("--all", Bool, "List every repository that keeps memory, with the global default")),
    ("memory status", m("--cwd", Path, "Any folder of the repository (default: the current folder)")),
    ("memory set", m("--cwd", Path, "Any folder of the repository; the switch is written at the repository root")),
    ("memory set", m("--global", Bool, "Set the global default in ~/.claude/settings.json instead of one repository")),
    ("memory set", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("memory clear", m("--cwd", Path, "Any folder of the repository (required)")),
    ("memory clear", m("--forever", Bool, "Delete for good instead of moving to the trash; needs --confirm with the repository name")),
    ("memory clear", m("--confirm", Str, "The repository name, typed, to confirm --forever")),
    ("memory clear", m("--also-off", Bool, "Also turn auto memory off for the repository")),
    ("memory clear", m("--dry-run", Bool, "List the files and change nothing")),
    ("memory undo", m("--cwd", Path, "Any folder of the repository, to pick its newest cleared memory")),
    ("memory undo", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("memory overview", m("--cwd", Path, "Any folder of the repository (default: the current folder)")),
    ("memory overview", m("--all", Bool, "One row per repository Toolport knows, with the folders that match none")),
    ("memory overview", m("--sort", Choice(&["name", "memory", "sessions", "size", "active"]), "Order of the rows (default: name)")),
    ("memory overview", m("--filter", Choice(&["memory-off", "has-local-file", "expiring", "large"]), "Keep only the repositories with memory off, a local instruction file, sessions expiring this week or more than 100 MB")),
    ("sessions ls", m("--cwd", Path, "Any folder of the repository (default: the current folder)")),
    ("sessions ls", m("--all", Bool, "Totals for every repository instead of one")),
    ("sessions clear", m("--cwd", Path, "Any folder of the repository (required)")),
    ("sessions clear", m("--ids", Str, "Session ids to clear, comma separated")),
    ("sessions clear", m("--older-than", Int, "Clear the sessions not written for more than this many days")),
    ("sessions clear", m("--to", Choice(&["trash", "archive"]), "Where the sessions go (default: trash, kept 30 days; the archive is kept until you delete it)")),
    ("sessions clear", m("--forever", Bool, "Delete for good; needs --confirm with the repository name")),
    ("sessions clear", m("--confirm", Str, "The repository name, typed, to confirm --forever")),
    ("sessions clear", m("--dry-run", Bool, "List the sessions and change nothing")),
    ("sessions undo", m("--cwd", Path, "Any folder of the repository, to pick its newest cleared sessions")),
    ("sessions undo", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("sessions restore", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("sessions retention", m("--set", Int, "Keep sessions this many days for every project (at least 1: Claude Code rejects 0)")),
    ("sessions retention", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("context bundle apply", m("--cwd", Path, "Folder where Claude Code starts (default: the current folder)")),
    ("context bundle undo", m("--cwd", Path, "Folder where Claude Code starts (default: the current folder)")),
    ("context bundle status", m("--cwd", Path, "Folder where Claude Code starts (default: the current folder)")),
    ("context bundle launch", m("--cwd", Path, "Folder the command is meant for; the settings file is the same everywhere")),
    ("context use", m("--cwd", Path, "Folder where Claude Code starts (default: the current folder)")),
    ("context bundle add", m("--from-folder", Path, "Start from what the folder's .claude/settings.local.json hides today")),
    ("context bundle add", m("--description", Str, "One line about the bundle")),
    ("context bundle add", m("--skills-off", Str, "Comma list of skills (globs allowed) the bundle turns off")),
    ("context bundle add", m("--skills-name-only", Str, "Comma list of skills the bundle keeps as a name without the description")),
    ("context bundle add", m("--skills-allow", Str, "Comma list of skills to keep; every other library skill is turned off")),
    ("context bundle add", m("--plugins-off", Str, "Comma list of plugin ids (name@marketplace) the bundle turns off")),
    ("context bundle add", m("--layers-add", Str, "Comma list of rule layers added to the folder's CLAUDE.local.md")),
    ("context bundle add", m("--layers-exclude", Str, "Comma list of CLAUDE.md globs added to claudeMdExcludes")),
    ("context bundle add", m("--agents-off", Str, "Comma list of agents the bundle denies")),
    ("context bundle add", m("--bind", Str, "Comma list of folder patterns the bundle is meant for (used by auto-apply)")),
    ("context bundle edit", m("--description", Str, "One line about the bundle")),
    ("context bundle edit", m("--skills-off", Str, "Comma list of skills (globs allowed) the bundle turns off")),
    ("context bundle edit", m("--skills-name-only", Str, "Comma list of skills the bundle keeps as a name without the description")),
    ("context bundle edit", m("--skills-allow", Str, "Comma list of skills to keep; every other library skill is turned off")),
    ("context bundle edit", m("--plugins-off", Str, "Comma list of plugin ids (name@marketplace) the bundle turns off")),
    ("context bundle edit", m("--layers-add", Str, "Comma list of rule layers added to the folder's CLAUDE.local.md")),
    ("context bundle edit", m("--layers-exclude", Str, "Comma list of CLAUDE.md globs added to claudeMdExcludes")),
    ("context bundle edit", m("--agents-off", Str, "Comma list of agents the bundle denies")),
    ("context bundle edit", m("--bind", Str, "Comma list of folder patterns the bundle is meant for (used by auto-apply)")),
    ("context bundle config", m("--auto-apply", Str, "on or off: whether context sync applies bound bundles")),
    ("context use", m("--none", Bool, "Drop the applied bundle instead of applying one")),
    ("context bundle add", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("context bundle edit", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("context bundle rm", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("context bundle apply", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("context bundle undo", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("context use", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("context leave-out", m("--dry-run", Bool, "Show the plan and change nothing")),
    ("context loads", m("--cwd", Path, "Folder where Claude Code starts (default: the current folder)")),
    ("context compose", m("--cwd", Path, "Folder where Claude Code starts (default: the current folder)")),
    ("context sync", m("--dry-run", Bool, "Show what would be applied and change nothing")),
    ("context measure", m("--cwd", Path, "Folder where Claude Code starts (default: the current folder)")),
    ("context loads", m("--measured", Bool, "Add the last measurement and mark the rows its InstructionsLoaded log confirms")),
    ("context status", m("--cwd", Path, "Folder where Claude Code starts (default: the current folder)")),
    ("context measure", m("--without", Str, "Also measure without plugin:<id> or skill:<name> (a glob is allowed); repeat for more")),
    ("context measure", m("--bundle", Str, "Also measure what the named bundle would leave: one request per variant")),
    ("context measure", m("--model", Str, "Model to ask (default haiku)")),
    ("context measure", m("--force", Bool, "Measure again instead of answering from the cache")),
    ("context measure", m("--yes", Bool, "Start the requests: they spend model tokens, so nothing starts without it")),
    ("server restart", m("--wait", Int, "Seconds to wait for the gateways to answer (default 20; 0 asks and does not wait)")),
    ("server restart", m("--if-idle", Bool, "Ask first how many calls are in flight and restart only when there are none, because a restart ends them")),
    (
        "mcp install",
        m(
            "--access-set",
            Str,
            "Access set to list the self-management server in, whatever was decided before (--profile is the old spelling)",
        ),
    ),
    (
        "mcp call",
        m(
            "--args",
            Str,
            "The tool's arguments as one JSON object, for values that are not secret",
        ),
    ),
    (
        "mcp call",
        m(
            "--args-stdin",
            Bool,
            "Read the tool's arguments as one JSON object from stdin; nothing reaches argv",
        ),
    ),
];

pub(super) fn meta_for(row: &str, flag: &str) -> Option<&'static Meta> {
    META.iter()
        .find(|(id, meta)| *id == row && meta.flag == flag)
        .map(|(_, meta)| meta)
}

fn fallback(flag: &Flag) -> Ty {
    if flag.is_greedy() {
        Ty::Paths
    } else if flag.is_whole_number() {
        Ty::Int
    } else if flag.takes_value() {
        Ty::Str
    } else {
        Ty::Bool
    }
}

pub(super) fn row_flags(id: &str, row: Option<&Row>) -> Vec<Flag> {
    let mut specs: Vec<&'static Spec> = row.map(|r| r.specs.to_vec()).unwrap_or_default();
    if let Some((spec, _)) = policy::sub_spec(id) {
        specs.push(spec);
    }
    let only = row.map_or(&[][..], |r| r.only);
    let mut flags: Vec<Flag> = Vec::new();
    for spec in specs {
        for flag in spec.flags {
            if (only.is_empty() || only.contains(&flag.name()))
                && !flags.iter().any(|f| f.name() == flag.name())
            {
                flags.push(*flag);
            }
        }
    }
    for flag in row.map_or(&[][..], |r| r.extra) {
        if !flags.iter().any(|f| f.name() == flag.name()) {
            flags.push(*flag);
        }
    }
    flags
}

fn flag_json(id: &str, row: Option<&Row>, flag: &Flag) -> Value {
    let meta = meta_for(id, flag.name());
    let ty = meta.map_or_else(|| fallback(flag), |m| m.ty);
    let mut value = json!({
        "name": flag.name(),
        "aliases": flag.aliases(),
        "valueType": ty.name(),
        "required": row.is_some_and(|r| r.requires.contains(&flag.name())),
        "repeatable": meta.is_some_and(|m| m.repeatable),
        "escalates": row.is_some_and(|r| r.escalators.contains(&flag.name())),
        "hidden": meta.is_some_and(|m| m.hidden),
        "sensitive": meta.is_some_and(|m| m.sensitive),
        "effect": meta.map_or("", |m| m.effect),
    });
    if let Ty::Choice(choices) = ty {
        value["choices"] = json!(choices);
    }
    value
}

fn max_operands(row: &Row, id: &str) -> Option<usize> {
    let mut limits: Vec<Option<usize>> =
        row.specs.iter().map(|spec| spec.operand_limit()).collect();
    if let Some((spec, _)) = policy::sub_spec(id) {
        limits.push(spec.operand_limit());
    }
    let mut max = Some(0);
    for limit in &limits {
        match (max, limit) {
            (_, None) => return None,
            (Some(a), Some(b)) => max = Some(a.max(*b)),
            (None, _) => return None,
        }
    }
    if limits.is_empty() {
        return None;
    }
    max
}

fn operand_json(operand: &Operand) -> Value {
    json!({
        "name": operand.name,
        "required": operand.required,
        "variadic": operand.variadic,
    })
}

fn preview_json(preview: Preview) -> Value {
    match preview {
        Preview::None => json!({"mode": "none", "flag": null}),
        Preview::Flag(flag) => json!({"mode": "flag", "flag": flag}),
        Preview::UnlessApplied(flag) => json!({"mode": "unless-applied", "flag": flag}),
    }
}

struct Entry {
    id: String,
    path: Vec<&'static str>,
    summary: &'static str,
    planned: bool,
}

fn entries() -> Vec<Entry> {
    let mut out = Vec::new();
    for command in COMMANDS {
        out.push(Entry {
            id: command.path.join(" "),
            path: command.path.to_vec(),
            summary: command.summary,
            planned: command.planned(),
        });
        for sub in SUBS.iter().filter(|s| s.parent == command.path.join(" ")) {
            let mut path = command.path.to_vec();
            path.push(sub.name);
            out.push(Entry {
                id: sub.id(),
                path,
                summary: sub.summary,
                planned: false,
            });
        }
    }
    out
}

fn parent_of(entry: &Entry, all: &[Entry]) -> Option<String> {
    (1..entry.path.len())
        .rev()
        .map(|n| entry.path[..n].join(" "))
        .find(|candidate| all.iter().any(|e| e.id == *candidate))
}

fn tools_for(command: &str) -> Vec<&'static str> {
    policy::TOOL_ROWS
        .iter()
        .filter(|t| t.command == Some(command))
        .map(|t| t.name)
        .collect()
}

fn command_json(entry: &Entry, all: &[Entry]) -> Value {
    let is_group = all
        .iter()
        .any(|e| e.path.len() > entry.path.len() && e.path.starts_with(&entry.path));
    let mut value = json!({
        "id": entry.id,
        "path": entry.path,
        "group": entry.path[0],
        "kind": if is_group { "group" } else { "command" },
        "parent": parent_of(entry, all),
        "summary": entry.summary,
        "planned": entry.planned,
    });
    let row = policy::row_of(&entry.id);
    let policy_fields = match row {
        Some(row) if !is_group => json!({
            "tier": row.tier.as_str(),
            "baseTier": if row.reads_by_default { Tier::Read } else { row.tier }.as_str(),
            "dryRun": row.preview != Preview::None,
            "preview": preview_json(row.preview),
            "needs": row.needs.iter().map(|n| n.as_str()).collect::<Vec<_>>(),
            "cost": row.cost,
            "surface": if row.surface == Surface::Terminal { "terminal" } else { "screen" },
            "operands": row.operands.iter().map(operand_json).collect::<Vec<_>>(),
            "maxOperands": max_operands(row, &entry.id),
            "flags": row_flags(&entry.id, Some(row)).iter().map(|f| flag_json(&entry.id, Some(row), f)).collect::<Vec<_>>(),
            "operandEscalates": row.operand_escalates,
            "oneOf": row.one_of,
            "tools": tools_for(&entry.id),
        }),
        _ => json!({
            "tier": null,
            "baseTier": null,
            "dryRun": false,
            "preview": null,
            "needs": [],
            "cost": false,
            "surface": null,
            "operands": [],
            "maxOperands": null,
            "flags": [],
            "operandEscalates": false,
            "oneOf": [],
            "tools": [],
        }),
    };
    if let (Some(target), Some(source)) = (value.as_object_mut(), policy_fields.as_object()) {
        target.extend(source.clone());
    }
    value
}

fn tool_json(def: &crate::selfmcp::ToolDef) -> Value {
    let row = policy::TOOL_ROWS.iter().find(|t| t.name == def.name);
    json!({
        "name": def.name,
        "tier": row.map(|t| t.tier.as_str()),
        "toolTier": def.tier,
        "dryRun": row.map_or(ToolPreview::None, |t| t.preview).as_str(),
        "command": row.and_then(|t| t.command),
    })
}

/// The data of `toolportctl commands --json`.
pub fn registry() -> Value {
    let all = entries();
    let commands: Vec<Value> = all.iter().map(|e| command_json(e, &all)).collect();
    let leaves = commands.iter().filter(|c| c["kind"] == "command").count();
    let needs = [
        Needs::Stdin,
        Needs::Browser,
        Needs::LongRunning,
        Needs::Network,
        Needs::TerminalOnly,
    ]
    .map(Needs::as_str);
    json!({
        "tiers": [Tier::Read.as_str(), Tier::Write.as_str(), Tier::Destructive.as_str()],
        "needs": needs,
        "counts": {
            "rows": commands.len(),
            "commands": leaves,
            "groups": commands.len() - leaves,
            "tools": TOOLS.len(),
        },
        "commands": commands,
        "tools": TOOLS.iter().map(tool_json).collect::<Vec<_>>(),
    })
}

pub fn run(_: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let data = registry();
    let mut human = format!(
        "{} commands in {} groups, {} self-management tools\n\
         tier: read changes nothing, write changes files, destructive deletes or overwrites; \
         * previews with --dry-run\n",
        data["counts"]["commands"], data["counts"]["groups"], data["counts"]["tools"]
    );
    for command in data["commands"].as_array().into_iter().flatten() {
        if command["kind"] != "command" {
            continue;
        }
        let tier = command["tier"].as_str().unwrap_or("");
        let preview = if command["dryRun"] == true { "*" } else { " " };
        human.push_str(&format!(
            "  {:<28} {:<11}{preview} {}\n",
            command["id"].as_str().unwrap_or(""),
            tier,
            command["summary"].as_str().unwrap_or("")
        ));
    }
    Ok(Output::new(data, human))
}
