//! `toolportctl` command line (D-010, D-058). The binary is a thin `main`; parsing, dispatch and
//! rendering live here so they are unit-testable. `--json` prints one envelope
//! (`ok`, `command`, `schemaVersion`, `data` or `error`) that the GUI bridge and scripts parse.
//!
//! A command is one row of [`COMMANDS`]. Its safety class, flags and operands are a row of
//! [`policy`], and `toolportctl commands` prints both, so the registry cannot drift from what a
//! handler accepts.

mod agents;
mod auth;
mod commands;
mod commands_json;
pub(crate) mod compression;
mod compression_cfg;
mod compression_hooks;
mod context;
mod flags;
mod library;
mod mcp;
mod output;
mod plugins;
mod policy;
mod secret;
mod server;
mod setup;
mod skills;
mod skills_checks;
mod skills_repo;
mod skills_state;
mod skills_taps;
mod skills_undo;
mod sources;
mod styles;
mod task;

use toolport_plus_api::Host;

pub use commands_json::registry;
pub use output::{CtlError, ErrorKind, Output};
pub use policy::{
    rows, terminal_only, tool_rows, Needs, Preview, RowView, Smoke, Surface, Tier, ToolPreview,
    ToolRowView,
};

use output::Envelope;

pub const SCHEMA_VERSION: u32 = 1;
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

pub type Handler = fn(&dyn Host, &[String]) -> Result<Output, CtlError>;

fn exit_code(kind: ErrorKind) -> i32 {
    match kind {
        ErrorKind::Usage => EXIT_USAGE,
        _ => EXIT_ERROR,
    }
}

pub struct Command {
    pub path: &'static [&'static str],
    pub summary: &'static str,
    pub handler: Handler,
    pub options: bool,
}

const fn cmd(path: &'static [&'static str], summary: &'static str, handler: Handler) -> Command {
    Command {
        path,
        summary,
        handler,
        options: true,
    }
}

impl Command {
    pub fn planned(&self) -> bool {
        self.summary.ends_with("(not implemented)")
    }

    const fn no_options(self) -> Self {
        Self {
            options: false,
            ..self
        }
    }
}

pub const COMMANDS: &[Command] = &[
    cmd(
        &["status"],
        "Show registry, access sets, secrets backend and gateway state",
        commands::status,
    )
    .no_options(),
    cmd(&["doctor"], "Run read-only health checks", commands::doctor).no_options(),
    cmd(
        &["commands"],
        "List all commands with tier, dry-run support and flags (the registry)",
        commands_json::run,
    )
    .no_options(),
    cmd(
        &["server", "ls"],
        "List servers with their global switch and default access",
        server::ls,
    )
    .no_options(),
    cmd(
        &["server", "info"],
        "Show one server: command, plain env keys, access sets and what its process sees",
        server::info,
    ),
    cmd(
        &["server", "install"],
        "Add a curated catalog server <name> [--offline]",
        server::install,
    ),
    cmd(
        &["server", "new"],
        "Add a server <name> from a command or a URL, with plain env and --inherit-env",
        server::new,
    ),
    cmd(
        &["server", "edit"],
        "Change a server's command, args, url, cwd, plain env or --inherit-env",
        server::edit,
    ),
    cmd(
        &["server", "mode"],
        "Choose whether a server's tools are listed (auto) or only searched (search): <server> auto|search",
        server::mode,
    ),
    cmd(
        &["server", "uninstall"],
        "Remove a server and its vault entries [--dry-run] [--keep-secrets]",
        server::uninstall,
    ),
    cmd(
        &["server", "restart"],
        "Restart a server's process in every running gateway [--dry-run] [--wait <s>] [--if-idle]",
        server::restart,
    ),
    cmd(
        &["auth", "statusline"],
        "Compact login-health JSON for a Claude Code statusline",
        auth::statusline,
    )
    .no_options(),
    cmd(
        &["auth", "hook"],
        "Login-health JSON for a Claude Code SessionStart hook",
        auth::hook,
    )
    .no_options(),
    cmd(
        &["auth", "probe"],
        "Probe login health now (--server <id>, --force)",
        auth::probe,
    ),
    cmd(
        &["auth", "login"],
        "Sign in to a server again (<server>, --no-open)",
        auth::login,
    ),
    cmd(
        &["secret", "set"],
        "Store a secret read from stdin or --value-env",
        secret::set,
    ),
    cmd(
        &["secret", "get"],
        "Check a secret (--reveal prints the value)",
        secret::get,
    ),
    cmd(&["secret", "rm"], "Remove a secret", secret::rm),
    cmd(
        &["context", "bundle", "ls"],
        "List the context bundles with where each is applied",
        context::ls,
    )
    .no_options(),
    cmd(
        &["context", "bundle", "show"],
        "Show one bundle's definition and lint (<name>)",
        context::show,
    )
    .no_options(),
    cmd(
        &["context", "bundle", "add"],
        "Write a new bundle (<name>, --from-folder, --description, --skills-off, --skills-name-only, --skills-allow, --plugins-off, --layers-add, --layers-exclude, --agents-off, --servers, --bind, --dry-run)",
        context::add,
    ),
    cmd(
        &["context", "bundle", "edit"],
        "Change a bundle; each given list replaces that list (<name>, same flags as add, --dry-run)",
        context::edit,
    ),
    cmd(
        &["context", "bundle", "rm"],
        "Delete a bundle definition; refuses while applied unless --force (<name>, --force, --dry-run)",
        context::rm,
    ),
    cmd(
        &["context", "bundle", "apply"],
        "Apply a bundle in the folder where Claude starts, through git-ignored local files (<name>, --cwd, --dry-run)",
        context::apply,
    ),
    cmd(
        &["context", "bundle", "undo"],
        "Undo the applied bundle in a folder; keys changed since are left alone (--cwd, --dry-run)",
        context::undo,
    ),
    cmd(
        &["context", "bundle", "status"],
        "Show the bundle applied in a folder and any drift (--cwd)",
        context::status,
    ),
    cmd(
        &["context", "bundle", "launch"],
        "Write the --settings file of a bundle and print the claude command (<name>, --cwd)",
        context::launch,
    ),
    cmd(
        &["context", "bundle", "config"],
        "Show or set whether context sync applies bound bundles (--auto-apply on|off)",
        context::config,
    ),
    cmd(
        &["context", "loads"],
        "Show what a Claude Code session in a folder loads, with an estimated token cost and the folder's calibration (--cwd, --measured)",
        context::loads,
    ),
    cmd(
        &["context", "status"],
        "Everything known about a folder's context, read only: bundle, leave-outs, plugin switches, last measurement (--cwd)",
        context::context_status,
    ),
    cmd(
        &["context", "measure"],
        "Measure what a folder really loads, in tokens, with Claude Code, as is and per variant (--cwd, --without plugin:<id>|skill:<name>, --bundle, --model, --force, --yes)",
        context::measure,
    ),
    cmd(
        &["context", "compose"],
        "Show the instruction text Claude gets in a folder, part by part in load order (--cwd)",
        context::compose,
    ),
    cmd(
        &["context", "sync"],
        "Apply the bound bundles when auto-apply is on (--dry-run); rule layers and shims are not built yet",
        context::sync,
    ),
    cmd(
        &["context", "use"],
        "Apply a bundle to a folder, or drop the applied one (<name>, --none, --cwd, --dry-run)",
        context::use_bundle,
    ),
    cmd(
        &["context", "leave-out"],
        "Leave a CLAUDE.md or rule out of a folder, or turn auto memory off there (add|rm <path> or --memory, --cwd, --dry-run)",
        context::leave_out,
    ),
    cmd(
        &["plugins", "off"],
        "Turn a plugin off in one folder: enabledPlugins in .claude/settings.local.json, undone by `plugins on` (<id>, --cwd, --dry-run)",
        plugins::off,
    ),
    cmd(
        &["plugins", "on"],
        "Turn a plugin back on in a folder: removes the entry `plugins off` wrote (<id>, --cwd, --dry-run)",
        plugins::on,
    ),
    cmd(
        &["skills", "sync"],
        "Transpile skills to client outputs (--repo, --home, --client, --project, --dry-run, --apply-held)",
        skills::sync,
    ),
    cmd(
        &["skills", "undo"],
        "Put back what the last sync changed from its snapshot and pause auto-sync (--repo, --home, --project, --dry-run)",
        skills_undo::undo,
    ),
    cmd(
        &["skills", "verify"],
        "Compare every file the lock records with the library; exits 1 on a mismatch (--repo, --home, --client, --project)",
        skills_undo::verify,
    ),
    cmd(
        &["skills", "auto", "status"],
        "Say whether the unattended sync is scheduled and paused, when it runs next and what its last run did",
        skills_undo::auto_status,
    ),
    cmd(
        &["skills", "auto", "on"],
        "Schedule the unattended sync every morning (--dry-run)",
        skills_undo::auto_on,
    ),
    cmd(
        &["skills", "auto", "off"],
        "Stop scheduling the unattended sync (--dry-run)",
        skills_undo::auto_off,
    ),
    cmd(
        &["skills", "auto", "pause"],
        "Pause the unattended sync until it is resumed",
        skills_undo::auto_pause,
    ),
    cmd(
        &["skills", "auto", "resume"],
        "Let the unattended sync run again",
        skills_undo::auto_resume,
    ),
    cmd(
        &["skills", "ls"],
        "List skills and rules (--repo <dir>, --home <dir>, --source <id>)",
        skills::ls,
    ),
    cmd(
        &["skills", "lint"],
        "Lint skills, skipped files and links; exits 1 on errors (--repo, --home, --name <skill>)",
        skills::lint,
    ),
    cmd(
        &["skills", "files"],
        "List every file of a skill with what the next sync does to it (<skill>, --repo, --home, --client <key>, --project)",
        skills_checks::files,
    ),
    cmd(
        &["skills", "fix"],
        "Apply the one safe lint fix: make hook scripts executable (chmod [<skill>...], --repo, --home, --dry-run)",
        skills_checks::fix,
    ),
    cmd(
        &["skills", "diff"],
        "Compare skills with the lockfile; exits 1 on changes (--repo, --home)",
        skills::diff,
    ),
    cmd(
        &["skills", "init"],
        "Create a skills repository (--path <dir>, --name <name>, --dry-run)",
        skills_repo::init,
    ),
    cmd(
        &["skills", "add"],
        "Create a skill or rule from a template (<name>, --type, --path, --with-progressive, --dry-run)",
        skills_repo::add,
    ),
    cmd(
        &["skills", "audit"],
        "Scan skills for prompt injection and risky commands; exits 1 on high findings (--path <dir>)",
        skills_repo::audit,
    ),
    cmd(
        &["skills", "tap", "add"],
        "Register and clone a tap (<user/repo|url>, --name <alias>, --dry-run)",
        skills_taps::tap_add,
    ),
    cmd(&["skills", "tap", "ls"], "List the registered taps", skills_taps::tap_ls),
    cmd(
        &["skills", "tap", "remove"],
        "Unregister a tap and delete its clone (<name>, --dry-run)",
        skills_taps::tap_remove,
    ),
    cmd(
        &["skills", "tap", "update"],
        "Pull one or all taps; exits 1 if one fails (<name>, --dry-run)",
        skills_taps::tap_update,
    ),
    cmd(
        &["skills", "search"],
        "Search the taps for skills by name, description or tags (<query>)",
        skills_taps::search,
    ),
    cmd(
        &["skills", "install"],
        "Install skills from a tap; a high-severity audit finding blocks it (<@user/repo[/skill]>, --path, --no-audit, --dry-run)",
        skills_taps::install,
    ),
    cmd(
        &["agents", "add"],
        "Create an agent from a template (<name>, --path, --dry-run)",
        agents::add,
    ),
    cmd(&["agents", "ls"], "List agents (--path <dir>)", agents::ls),
    cmd(
        &["agents", "lint"],
        "Lint agents; exits 1 on errors (--path <dir>)",
        agents::lint,
    ),
    cmd(
        &["agents", "audit"],
        "Scan agents for prompt injection and risky commands; exits 1 on high findings (--path <dir>)",
        agents::audit,
    ),
    cmd(
        &["agents", "diff"],
        "Show new, modified and removed agents since the last sync (--path <dir>)",
        agents::diff,
    ),
    cmd(
        &["agents", "status"],
        "Show whether synced agent outputs still exist; --strict exits 1 on drift (--path, --home)",
        agents::status,
    ),
    cmd(
        &["agents", "clean"],
        "Remove synced agent outputs, keeping the lockfile (--path, --home, --client <key>, --project, --dry-run)",
        agents::clean,
    ),
    cmd(
        &["agents", "uninstall"],
        "Remove an agent, its outputs and its lock entry (<name>, --path, --home, --project, --dry-run)",
        agents::uninstall,
    ),
    cmd(
        &["agents", "sync"],
        "Transpile agents to client outputs (--path, --home, --client <key>, --project, --dry-run)",
        agents::sync,
    ),
    cmd(
        &["styles", "add"],
        "Create an output style from a template (<name>, --path, --dry-run)",
        styles::add,
    ),
    cmd(
        &["styles", "ls"],
        "List output styles and where they are synced (--path <dir>)",
        styles::ls,
    ),
    cmd(
        &["styles", "lint"],
        "Lint output styles; exits 1 on errors (--path <dir>)",
        styles::lint,
    ),
    cmd(
        &["styles", "diff"],
        "Show new, modified and removed styles since the last sync (--path <dir>)",
        styles::diff,
    ),
    cmd(
        &["styles", "status"],
        "Show the synced and the active styles per client (--path <dir>)",
        styles::status,
    ),
    cmd(
        &["styles", "sync"],
        "Write every style to the native-toggle clients (--path, --home, --client <key>, --project, --dry-run)",
        styles::sync,
    ),
    cmd(
        &["styles", "apply"],
        "Apply one style as an always-on rule to the other clients (<name>, --path, --home, --client <key>, --project, --dry-run)",
        styles::apply,
    ),
    cmd(
        &["styles", "remove"],
        "Remove the active style from the other clients (--path, --home, --client <key>, --project, --dry-run)",
        styles::remove,
    ),
    cmd(
        &["styles", "clean"],
        "Remove every synced and applied style file (--path, --home, --project, --dry-run)",
        styles::clean,
    ),
    cmd(
        &["sources", "ls"],
        "List where skills, commands, agents, rules and CLAUDE.md come from (--source, --kind, --items, --cwd, --root, --deep, --refresh)",
        sources::ls,
    ),
    cmd(
        &["sources", "root", "ls"],
        "List the folders searched for repo checkouts",
        sources::root_ls,
    ),
    cmd(
        &["sources", "root", "add"],
        "Search a folder for repo checkouts (<dir>, --dry-run)",
        sources::root_add,
    ),
    cmd(
        &["sources", "root", "rm"],
        "Stop searching a folder (<dir>, --dry-run)",
        sources::root_rm,
    ),
    cmd(
        &["library", "status"],
        "Show the skills library against its remote: ahead, behind, uncommitted, credentials, duplicate clones (--fetch contacts the remote)",
        library::status,
    ),
    cmd(
        &["library", "pull"],
        "Fast-forward the skills library from its remote; refuses over uncommitted changes (--dry-run, --review adds the sync plan)",
        library::pull,
    ),
    cmd(
        &["library", "push"],
        "Audit and secret-scan, then push the skills library; never forces (--dry-run)",
        library::push,
    ),
    cmd(
        &["skills", "bundle"],
        "Pack skills into a portable zip (--output <zip>, --path <dir>, --skills <a,b>, --dry-run)",
        skills_repo::bundle,
    ),
    cmd(
        &["skills", "unbundle"],
        "Extract a skills bundle (<bundle.zip>, --path <dir>, --dry-run)",
        skills_repo::unbundle,
    ),
    cmd(
        &["skills", "status"],
        "Show whether synced outputs still exist and match the library; --strict exits 1 on drift (--repo, --home, --client <key>)",
        skills_state::status,
    ),
    cmd(
        &["skills", "clean"],
        "Remove synced outputs and the lockfile (--repo, --home, --client <key>, --project, --dry-run)",
        skills_state::clean,
    ),
    cmd(
        &["skills", "uninstall"],
        "Remove a skill, its outputs and its lock entry (<name>, --repo, --home, --project, --dry-run)",
        skills_state::uninstall,
    ),
    cmd(
        &["skills", "resolve"],
        "Find files shadowing synced skills; --migrate backs them up (--repo, --home, --client <key>, --project, --dry-run)",
        skills_state::resolve,
    ),
    cmd(
        &["compression", "status"],
        "Show the compression policy, pin and drift",
        compression::status,
    ),
    cmd(
        &["compression", "presets"],
        "List presets (--refresh re-snapshots knobs, --dry-run)",
        compression::presets,
    ),
    cmd(
        &["compression", "enable"],
        "Enable a provider (--provider, --port, --telemetry, --preset, --mode, --dry-run)",
        compression_cfg::enable,
    ),
    cmd(
        &["compression", "disable"],
        "Disable compression (--teardown, --dry-run)",
        compression_cfg::disable,
    ),
    cmd(
        &["compression", "set-provider"],
        "Swap the active provider and re-apply (--dry-run)",
        compression_cfg::set_provider,
    ),
    cmd(
        &["compression", "use"],
        "Switch the active preset and re-apply (--dry-run)",
        compression_cfg::use_preset,
    ),
    cmd(
        &["compression", "sync"],
        "Re-apply the policy: shims, env snippet, MCP entry (--mcpm-root, --dry-run)",
        compression_cfg::sync,
    ),
    cmd(
        &["compression", "pin"],
        "Show or set the exact engine pin (--install, --refresh, --dry-run)",
        compression_cfg::pin,
    ),
    cmd(
        &["compression", "seal"],
        "Declare the live proxy posture as policy (--apply, --dry-run)",
        compression_cfg::seal,
    ),
    cmd(
        &["compression", "env"],
        "Print the proxy env for a directory, for eval (--cwd)",
        compression_cfg::env,
    ),
    cmd(
        &["compression", "doctor"],
        "Run the compression health checks",
        compression_cfg::doctor,
    ),
    cmd(
        &["compression", "run"],
        "Launch claude under the directory's policy (--plan to preview)",
        compression::run,
    ),
    cmd(
        &["compression", "verify"],
        "Check provider health, pin, shims and measure cache behaviour",
        compression::verify,
    ),
    cmd(
        &["compression", "measure"],
        "Four honest questions about compression, each with its population (--transcripts, --min-turns, --since, --format, --out)",
        compression::measure,
    ),
    cmd(
        &["compression", "hooks"],
        "Own the rtk and read-once hook entries: status apply undo pause resume adopt-edit verify (--dry-run, --provider, --for, --until, --revert)",
        compression_hooks::hooks,
    ),
    cmd(
        &["compression", "hook"],
        "The wrapper a hook entry runs: <provider> -- <command...> (counts the call, passes the answer through)",
        compression_hooks::hook,
    ),
    cmd(
        &["compression", "ledger"],
        "Launch and token-savings ledger (summary | record)",
        compression::ledger_cmd,
    ),
    cmd(
        &["compression", "proxy"],
        "Proxy lifecycle: up, down, restart",
        compression::proxy,
    ),
    cmd(
        &["compression", "update"],
        "Move the engine pin (--to V | --latest, --accept to apply)",
        compression::update,
    ),
    cmd(
        &["compression"],
        "Compression: status presets enable disable set-provider use sync pin seal env doctor run verify measure ledger proxy update",
        compression_cfg::group,
    ),
    cmd(
        &["task", "ls"],
        "List the tasks with their last run and next scheduled run (--all includes disabled ones)",
        task::ls,
    ),
    cmd(
        &["task", "show"],
        "Show a task's definition and its last runs (<id>)",
        task::show,
    ),
    cmd(
        &["task", "run"],
        "Start a task; --dry-run lists the steps, secrets and servers (<id>, --dry-run, --wait, --yes)",
        task::run,
    ),
    cmd(
        &["task", "resume"],
        "Continue a run that waits for you (<run-id>)",
        task::resume,
    ),
    cmd(
        &["task", "cancel"],
        "Stop a run and its child process (<run-id>)",
        task::cancel,
    ),
    cmd(
        &["task", "add"],
        "Add a task from a definition file or a Claude command file (<id>, --file, --from-command, --dry-run)",
        task::add,
    ),
    cmd(
        &["task", "edit"],
        "Replace a task's definition (<id>, --file, --dry-run)",
        task::edit,
    ),
    cmd(
        &["task", "rm"],
        "Remove a task; its run history stays (<id>, --dry-run)",
        task::rm,
    ),
    cmd(
        &["task", "history"],
        "List the runs of a task, or one run with its step logs ([<id>], --run, --limit)",
        task::history,
    ),
    cmd(
        &["task", "missed"],
        "List the scheduled slots that did not start a run on time, newest first ([<id>])",
        task::missed,
    ),
    cmd(
        &["task", "enable"],
        "Turn a task on; its schedule counts from now (<id>, --dry-run)",
        task::enable,
    ),
    cmd(
        &["task", "disable"],
        "Turn a task off; nothing runs it until it is turned on (<id>, --dry-run)",
        task::disable,
    ),
    cmd(
        &["setup", "parts"],
        "List what a setup file can hold, with counts and sizes: repositories, servers, access sets, skills library, tasks, settings, Claude Code user files, memory, sessions, secrets (--roots <dir>...)",
        setup::parts,
    ),
    cmd(
        &["setup", "export"],
        "Write one zip with a manifest holding the parts you tick; secrets and sessions only sealed with a passphrase from stdin (--parts, --out, --name, --secrets, --sessions, --passphrase-stdin, --roots, --no-worktrees, --skip-no-remote, --library-zip, --without, --session-repo, --peek, --dry-run)",
        setup::export,
    ),
    cmd(
        &["setup", "inspect"],
        "Read a setup file without changing anything: parts, counts, hashes and what is damaged (<file>)",
        setup::inspect,
    ),
    cmd(
        &["setup", "restore"],
        "Plan a restore of a setup file against this Mac (dry run); apply it with the plan hash. Existing servers, repositories and files are kept unless a row is taken (<file>, --parts, --take, --passphrase-stdin, --apply, --plan)",
        setup::restore,
    ),
    cmd(
        &["setup", "undo"],
        "Undo the last restore run as far as nothing changed since (<run>, --dry-run)",
        setup::undo,
    ),
    cmd(
        &["setup", "check"],
        "Before a reset: unpushed work, repositories without a remote, changes and stashes, plus the things you carry yourself (--roots, --tick, --untick, --ack)",
        setup::check,
    ),
    cmd(
        &["mcp"],
        "Self-management server: install [--access-set <id>] uninstall doctor tools call <tool> [--args <json> | --args-stdin]",
        mcp::run,
    ),
];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub json: bool,
    pub help: bool,
    pub version: bool,
    pub data_dir: Option<String>,
    pub positional: Vec<String>,
}

pub fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut parsed = Parsed::default();
    let mut iter = args.iter();
    let mut rest_positional = false;
    while let Some(arg) = iter.next() {
        if rest_positional || !arg.starts_with('-') || arg == "-" {
            parsed.positional.push(arg.clone());
            continue;
        }
        match arg.as_str() {
            "--" => {
                rest_positional = true;
                if parsed.positional.first().map(String::as_str) == Some("compression") {
                    parsed.positional.push(arg.clone());
                }
            }
            "--json" => parsed.json = true,
            "-h" | "--help" => parsed.help = true,
            "-V" | "--version" => parsed.version = true,
            "--data-dir" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--data-dir requires a value".to_string())?;
                parsed.data_dir = Some(value.clone());
            }
            other => match other.strip_prefix("--data-dir=") {
                Some("") => return Err("--data-dir requires a value".into()),
                Some(value) => parsed.data_dir = Some(value.to_string()),
                None if takes_options(&parsed.positional) => {
                    parsed.positional.push(other.to_string())
                }
                None => return Err(format!("unknown option: {other}")),
            },
        }
    }
    Ok(parsed)
}

fn takes_options(positional: &[String]) -> bool {
    find_command(positional).is_some_and(|(command, _)| command.options)
}

pub fn find_command(positional: &[String]) -> Option<(&'static Command, &[String])> {
    COMMANDS
        .iter()
        .filter(|c| {
            positional.len() >= c.path.len()
                && c.path.iter().zip(positional).all(|(a, b)| *a == b.as_str())
        })
        .max_by_key(|c| c.path.len())
        .map(|c| (c, &positional[c.path.len()..]))
}

pub fn usage() -> String {
    let mut text = String::from(
        "toolportctl: manage Toolport+ from the command line\n\n\
         Usage: toolportctl [--json] [--data-dir <dir>] <command> [args]\n\n\
         Commands:\n",
    );
    for command in COMMANDS {
        let name = command.path.join(" ");
        text.push_str(&format!("  {name:<14} {}\n", command.summary));
    }
    text.push_str(
        "\nOptions:\n  --json             Machine-readable envelope on stdout\n  \
         --data-dir <dir>   Override the data directory (default: TOOLPORT_DATA_DIR)\n  \
         -h, --help         Show this help\n  -V, --version      Show the version\n\n\
         Exit codes: 0 ok, 1 error, 2 usage\n",
    );
    text
}

/// Runs the CLI and returns the process exit code.
pub fn run(host: &dyn Host, args: &[String]) -> i32 {
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    run_with(host, args, &mut out, &mut err)
}

pub fn run_with(
    host: &dyn Host,
    args: &[String],
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    let want_json = args.iter().any(|a| a == "--json");
    let parsed = match parse(args) {
        Ok(p) => p,
        Err(message) => return emit_usage(want_json, "", &message, out, err),
    };
    if parsed.help || (parsed.positional.is_empty() && !parsed.version) {
        let _ = write!(out, "{}", usage());
        return if parsed.help { EXIT_OK } else { EXIT_USAGE };
    }
    if parsed.version {
        return emit(
            parsed.json,
            "version",
            Ok(Output::new(
                serde_json::json!({"name": "toolportctl", "version": host.app_version()}),
                format!("toolportctl {}", host.app_version()),
            )),
            out,
            err,
        );
    }
    let name = parsed.positional.join(" ");
    let Some((command, rest)) = find_command(&parsed.positional) else {
        return emit_usage(
            parsed.json,
            &name,
            &format!("unknown command: {name}"),
            out,
            err,
        );
    };
    if task::is_runner(&parsed.positional) {
        if let Some(dir) = &parsed.data_dir {
            host.use_data_dir(std::path::Path::new(dir));
        }
        return task::runner_main(host, &parsed.positional);
    }
    let command_name = command.path.join(" ");
    if let Some(dir) = &parsed.data_dir {
        host.use_data_dir(std::path::Path::new(dir));
    }
    emit(
        parsed.json,
        &command_name,
        (command.handler)(host, rest),
        out,
        err,
    )
}

fn emit_usage(
    json: bool,
    command: &str,
    message: &str,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    emit(json, command, Err(CtlError::usage(message)), out, err)
}

fn emit(
    json: bool,
    command: &str,
    result: Result<Output, CtlError>,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    let (code, envelope, human) = match result {
        Ok(output) => {
            let code = if output.failed { EXIT_ERROR } else { EXIT_OK };
            let envelope = if output.failed {
                Envelope::failure_with_data(
                    command,
                    CtlError::unhealthy("one or more checks failed"),
                    output.data,
                )
            } else {
                Envelope::success(command, output.data)
            };
            (code, envelope, output.human)
        }
        Err(error) => {
            let code = exit_code(error.kind);
            let human = if error.kind == ErrorKind::Usage {
                format!(
                    "toolportctl: {}\nRun `toolportctl --help` for usage.",
                    error.message
                )
            } else {
                format!("toolportctl: {}", error.message)
            };
            (code, Envelope::failure(command, error), human)
        }
    };
    if json {
        let line = serde_json::to_string(&envelope.to_value()).unwrap_or_default();
        let _ = writeln!(out, "{line}");
    } else if code == EXIT_OK
        || envelope.error.as_ref().map(|e| e.kind) == Some(ErrorKind::Unhealthy)
    {
        let _ = writeln!(out, "{}", human.trim_end());
    } else {
        let _ = writeln!(err, "{}", human.trim_end());
    }
    code
}

#[cfg(test)]
mod compression_cfg_tests;
#[cfg(test)]
mod compression_golden_tests;
#[cfg(test)]
mod compression_tests;
#[cfg(test)]
mod tests;
