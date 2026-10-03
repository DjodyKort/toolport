//! `toolportctl` command line (D-010, MIG-SELF). The binary is a thin `main`;
//! parsing, dispatch and rendering live here so they are unit-testable.

mod auth;
mod cc;
mod client;
mod commands;
pub(crate) mod compression;
mod compression_cfg;
mod council;
mod import;
mod skills;
mod skills_repo;
mod skills_state;
pub(crate) mod context;
mod flags;
mod folders;
mod mcp;
mod output;
mod secret;
mod server;
mod sync;
mod update;
mod usage;

pub use output::ErrorKind;
use output::{CtlError, Envelope, Output};

pub const SCHEMA_VERSION: u32 = 1;
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

pub type Handler = fn(&[String]) -> Result<Output, CtlError>;

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

macro_rules! planned_groups {
    ($($name:ident => $label:literal),* $(,)?) => {$(
        fn $name(_: &[String]) -> Result<Output, CtlError> {
            Err(CtlError::not_implemented(concat!($label, ": not implemented"),
            ))
        }
    )*};
}

planned_groups!(
    profile_group => "profile",
    client_group => "client",
    server_group => "server",
    auth_group => "auth",
    secret_group => "secret",
    import_group => "import",
);

pub const COMMANDS: &[Command] = &[
    cmd(
        &["status"],
        "Show registry, profile, secrets backend and gateway state",
        commands::status,
    )
    .no_options(),
    cmd(&["doctor"], "Run read-only health checks", commands::doctor).no_options(),
    cmd(
        &["server", "ls"],
        "List servers and whether the active profile enables them",
        commands::server_ls,
    ),
    cmd(&["server", "search"], "Search the catalog (--offline, --limit <n>)", server::search),
    cmd(&["server", "install"], "Install a catalog server by name", server::install),
    cmd(
        &["server", "uninstall"],
        "Remove a server and its client entries (--dry-run)",
        server::uninstall,
    ),
    cmd(&["server", "info"], "Show one server's definition and profiles", server::info),
    cmd(&["server", "new"], "Add a custom server (--command or --url)", server::new),
    cmd(&["server", "edit"], "Edit a server's name, command, args, url or cwd", server::edit),
    cmd(&["inspect"], "List the tools a server exposes (connects live)", server::inspect),
    cmd(
        &["profile", "inspect"],
        "List the tools of every server in a profile (connects live)",
        server::profile_inspect,
    ),
    cmd(&["profile"], "Profiles (not implemented)", profile_group),
    cmd(&["client", "ls"], "List detected clients and their direct entries", client::ls),
    cmd(
        &["client", "sync"],
        "Sync managed clients (--client <id>, --dry-run, --keep-orphans)",
        client::sync,
    ),
    cmd(&["client"], "Client configs (not implemented)", client_group),
    cmd(&["server"], "Manage servers (mutations) (not implemented)", server_group),
    cmd(
        &["auth", "statusline"],
        "Compact auth-health JSON for a Claude Code statusline",
        auth::statusline,
    )
    .no_options(),
    cmd(
        &["auth", "hook"],
        "Auth-health JSON for a Claude Code SessionStart hook",
        auth::hook,
    )
    .no_options(),
    cmd(&["auth"], "Auth health (not implemented)", auth_group).no_options(),
    cmd(&["secret", "set"], "Store a secret read from stdin or --value-env", secret::set),
    cmd(&["secret", "get"], "Check a secret (--reveal prints the value)", secret::get),
    cmd(&["secret", "rm"], "Remove a secret", secret::rm),
    cmd(&["secret"], "Manage server secrets (not implemented)", secret_group),
    cmd(
        &["context", "loads"],
        "Show what a claude session loads, with token cost (--profile, --cwd)",
        context::loads,
    ),
    cmd(
        &["context", "folders"],
        "Show the active profile per folder (--cwd, --enable, --disable)",
        folders::folders,
    ),
    cmd(
        &["context", "checkpoint-status"],
        "Report used tokens vs the checkpoint point from statusline JSON on stdin",
        context::checkpoint_status,
    ),
    cmd(&["context", "plan"], "Preview the context deploy (--home <dir>, --rules)", context::plan),
    cmd(
        &["context", "apply"],
        "Apply the context deploy (--home <dir>, --rules, --no-persist, --dry-run)",
        context::apply,
    ),
    cmd(
        &["context", "sync"],
        "Plan then apply the context deploy (--home <dir>, --rules, --dry-run)",
        context::sync,
    ),
    cmd(&["context"], "Context: loads checkpoint-status plan apply sync", context::group),
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
        &["compression", "ledger"],
        "Launch and token-savings ledger (summary | record)",
        compression::ledger_cmd,
    ),
    cmd(&["compression", "proxy"], "Proxy lifecycle: up, down, restart", compression::proxy),
    cmd(
        &["compression", "update"],
        "Move the engine pin (--to V | --latest, --accept to apply)",
        compression::update,
    ),
    cmd(
        &["compression"],
        "Compression: status presets enable disable set-provider use sync pin seal env doctor run verify ledger proxy update",
        compression_cfg::group,
    ),
    cmd(
        &["import", "mcpm"],
        "Import an mcpm config root (--dry-run prints the plan)",
        import::mcpm,
    ),
    cmd(
        &["import", "rename-refs"],
        "Rewrite mcp__mcpm_ tool references under --paths (--tools F, --dry-run)",
        import::rename_refs_cmd,
    ),
    cmd(&["import"], "Import data from other tools (not implemented)", import_group),
    cmd(&["council"], "Council server: install uninstall doctor tools", council::run),
    cmd(
        &["mcp"],
        "Self-management server: install [--profile <id>] uninstall doctor tools",
        mcp::run,
    ),
    cmd(
        &["skills", "sync"],
        "Transpile skills to client outputs (--repo, --home, --client, --project, --dry-run)",
        skills::sync,
    ),
    cmd(&["skills", "ls"], "List skills and rules (--repo <dir>, --home <dir>)", skills::ls),
    cmd(
        &["skills", "lint"],
        "Lint skills; exits 1 on errors (--repo, --home, --name <skill>)",
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
        "Show whether synced outputs still exist; --strict exits 1 on drift (--repo, --home, --client <key>)",
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
        &["skills"],
        "Skills: init add ls lint audit bundle unbundle sync diff status clean uninstall resolve",
        skills::group,
    ),
    cmd(&["sync"], "Encrypted sync: init push pull diff status reset ...", sync::run),
    cmd(&["cc"], "Claude Code plugins: list | update [--dry-run]", cc::run),
    cmd(
        &["update"],
        "Check or apply server updates (--check, --apply, --init, --dry-run)",
        update::update,
    ),
    cmd(
        &["usage"],
        "Token and MCP usage from Claude Code transcripts (--root <dir>, --no-refresh)",
        usage::run,
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
                None if takes_options(parsed.positional.first()) => {
                    parsed.positional.push(other.to_string())
                }
                None => return Err(format!("unknown option: {other}")),
            },
        }
    }
    Ok(parsed)
}

fn takes_options(first: Option<&String>) -> bool {
    first.is_some_and(|word| {
        COMMANDS
            .iter()
            .any(|c| c.options && c.path[0] == word.as_str())
    })
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
    if let Some(dir) = &parsed.data_dir {
        // Must precede the first data-dir lookup, which is memoized.
        std::env::set_var("TOOLPORT_DATA_DIR", dir);
    }
    emit(parsed.json, &command_name, (command.handler)(rest), out, err)
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
            let code = error.kind.exit_code();
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
    } else if code == EXIT_OK || envelope.error.as_ref().map(|e| e.kind) == Some(ErrorKind::Unhealthy)
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
mod compression_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod usage_pins_tests;

#[cfg(test)]
mod server_tests;

#[cfg(test)]
mod import_tests;

#[cfg(test)]
mod skills_tests;
