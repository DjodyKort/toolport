//! `toolportctl` command line (D-010, MIG-SELF). The binary is a thin `main`;
//! parsing, dispatch and rendering live here so they are unit-testable.

mod auth;
mod commands;
mod compression;
mod council;
mod import;
pub(crate) mod context;
mod output;
mod secret;
mod sync;
mod update;

use output::{CtlError, Envelope, Output};

pub const SCHEMA_VERSION: u32 = 1;
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

pub type Handler = fn(&[String]) -> Result<Output, CtlError>;

/// One row per command path. `None` marks a planned command that later items
/// fill in by swapping the handler; matching prefers the longest path.
pub struct Command {
    pub path: &'static [&'static str],
    pub summary: &'static str,
    pub handler: Option<Handler>,
}

pub const COMMANDS: &[Command] = &[
    Command {
        path: &["status"],
        summary: "Show registry, profile, secrets backend and gateway state",
        handler: Some(commands::status),
    },
    Command {
        path: &["doctor"],
        summary: "Run read-only health checks",
        handler: Some(commands::doctor),
    },
    Command {
        path: &["server", "ls"],
        summary: "List servers and whether the active profile enables them",
        handler: Some(commands::server_ls),
    },
    Command {
        path: &["server"],
        summary: "Manage servers (mutations)",
        handler: None,
    },
    Command {
        path: &["auth", "statusline"],
        summary: "Compact auth-health JSON for a Claude Code statusline",
        handler: Some(auth::statusline),
    },
    Command {
        path: &["auth", "hook"],
        summary: "Auth-health JSON for a Claude Code SessionStart hook",
        handler: Some(auth::hook),
    },
    Command {
        path: &["auth"],
        summary: "Auth health",
        handler: None,
    },
    Command {
        path: &["secret", "set"],
        summary: "Store a secret read from stdin or --value-env",
        handler: Some(secret::set),
    },
    Command {
        path: &["secret", "get"],
        summary: "Check a secret (--reveal prints the value)",
        handler: Some(secret::get),
    },
    Command {
        path: &["secret", "rm"],
        summary: "Remove a secret",
        handler: Some(secret::rm),
    },
    Command {
        path: &["secret"],
        summary: "Manage server secrets",
        handler: None,
    },
    Command {
        path: &["context", "loads"],
        summary: "Show what a claude session loads, with token cost (--profile, --cwd)",
        handler: Some(context::loads),
    },
    Command {
        path: &["context", "checkpoint-status"],
        summary: "Report used tokens vs the checkpoint point from statusline JSON on stdin",
        handler: Some(context::checkpoint_status),
    },
    Command {
        path: &["context"],
        summary: "Context sync",
        handler: None,
    },
    Command {
        path: &["compression", "status"],
        summary: "Show the compression policy, pin and drift",
        handler: Some(compression::status),
    },
    Command {
        path: &["compression", "presets"],
        summary: "List compression presets",
        handler: Some(compression::presets),
    },
    Command {
        path: &["compression", "run"],
        summary: "Launch claude under the directory's policy (--plan to preview)",
        handler: Some(compression::run),
    },
    Command {
        path: &["compression", "verify"],
        summary: "Check provider health, pin, shims and measure cache behaviour",
        handler: Some(compression::verify),
    },
    Command {
        path: &["compression", "ledger"],
        summary: "Launch and token-savings ledger (summary | record)",
        handler: Some(compression::ledger_cmd),
    },
    Command {
        path: &["compression", "proxy"],
        summary: "Proxy lifecycle: up, down, restart",
        handler: Some(compression::proxy),
    },
    Command {
        path: &["compression", "update"],
        summary: "Move the engine pin (--to V | --latest, --accept to apply)",
        handler: Some(compression::update),
    },
    Command {
        path: &["compression"],
        summary: "Compression runs",
        handler: None,
    },
    Command {
        path: &["import", "mcpm"],
        summary: "Import an mcpm config root (--dry-run prints the plan)",
        handler: Some(import::mcpm),
    },
    Command {
        path: &["import"],
        summary: "Import data from other tools",
        handler: None,
    },
    Command {
        path: &["council"],
        summary: "Council server: install uninstall doctor tools",
        handler: Some(council::run),
    },
    Command {
        path: &["skills"],
        summary: "Skills sync",
        handler: None,
    },
    Command {
        path: &["sync"],
        summary: "Encrypted sync: init push pull diff status reset ...",
        handler: Some(sync::run),
    },
    Command {
        path: &["update"],
        summary: "Check or apply server updates (--check, --apply, --init, --dry-run)",
        handler: Some(update::update),
    },
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
                None if matches!(
                    parsed.positional.first().map(String::as_str),
                    Some("compression" | "secret" | "import" | "context" | "sync" | "update" | "council")
                ) =>
                {
                    parsed.positional.push(other.to_string())
                }
                None => return Err(format!("unknown option: {other}")),
            },
        }
    }
    Ok(parsed)
}

pub fn find_command<'a>(positional: &'a [String]) -> Option<(&'static Command, &'a [String])> {
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
        let suffix = if command.handler.is_none() {
            " (not implemented)"
        } else {
            ""
        };
        text.push_str(&format!("  {name:<14} {}{suffix}\n", command.summary));
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
pub fn run(args: &[String]) -> i32 {
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    run_with(args, &mut out, &mut err)
}

pub fn run_with(
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
                serde_json::json!({"name": "toolportctl", "version": env!("CARGO_PKG_VERSION")}),
                format!("toolportctl {}", env!("CARGO_PKG_VERSION")),
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
    let Some(handler) = command.handler else {
        return emit(
            parsed.json,
            &command_name,
            Err(CtlError::new(
                "not_implemented",
                format!("{command_name}: not implemented"),
            )),
            out,
            err,
        );
    };
    if let Some(dir) = &parsed.data_dir {
        // Must precede the first data-dir lookup, which is memoized.
        std::env::set_var("TOOLPORT_DATA_DIR", dir);
    }
    emit(parsed.json, &command_name, handler(rest), out, err)
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
                    CtlError::new("unhealthy", "one or more checks failed"),
                    output.data,
                )
            } else {
                Envelope::success(command, output.data)
            };
            (code, envelope, output.human)
        }
        Err(error) => {
            let code = if error.code == "usage" {
                EXIT_USAGE
            } else {
                EXIT_ERROR
            };
            let human = if error.code == "usage" {
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
    } else if code == EXIT_OK {
        let _ = writeln!(out, "{}", human.trim_end());
    } else if envelope.error.as_ref().map(|e| e.code.as_str()) == Some("unhealthy") {
        let _ = writeln!(out, "{}", human.trim_end());
    } else {
        let _ = writeln!(err, "{}", human.trim_end());
    }
    code
}

#[cfg(test)]
mod compression_tests;
#[cfg(test)]
mod tests;
