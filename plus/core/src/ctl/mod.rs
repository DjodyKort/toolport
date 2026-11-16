//! `toolportctl` command line (D-010, D-058). The binary is a thin `main`; parsing, dispatch and
//! rendering live here so they are unit-testable. `--json` prints one envelope
//! (`ok`, `command`, `schemaVersion`, `data` or `error`) that the GUI bridge and scripts parse.
//!
//! A command is one row of [`COMMANDS`]. Its safety class, flags and operands are a row of
//! [`policy`], and `toolportctl commands` prints both, so the registry cannot drift from what a
//! handler accepts.

mod commands;
mod commands_json;
mod flags;
mod mcp;
mod output;
mod policy;
mod secret;
mod server;

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
