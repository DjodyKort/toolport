//! `toolportctl` command line (D-010, D-058). The binary is a thin `main`; parsing, dispatch and
//! rendering live here so they are unit-testable. `--json` prints one envelope
//! (`ok`, `command`, `schemaVersion`, `data` or `error`) that the GUI bridge and scripts parse.
//!
//! A command is one row of [`COMMANDS`]. Its safety class, flags and operands are a row of
//! [`policy`], and `toolportctl commands` prints both, so the registry cannot drift from what a
//! handler accepts.

mod auth;
mod commands;
mod commands_json;
mod context;
mod flags;
mod mcp;
mod output;
mod plugins;
mod policy;
mod secret;
mod server;
mod skills;
mod skills_repo;
mod skills_state;

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
        "Apply a bundle and its paired access set to a folder, or drop both (<name>, --none, --cwd, --dry-run)",
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
            "--" => rest_positional = true,
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
mod tests;
