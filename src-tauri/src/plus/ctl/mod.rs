//! `toolportctl` command line (D-010, MIG-SELF). The binary is a thin `main`;
//! parsing, dispatch and rendering live here so they are unit-testable.

mod agents;
mod auth;
mod cc;
mod client;
mod client_direct;
mod client_edit;
mod commands;
mod commands_json;
pub(crate) mod compression;
mod compression_cfg;
mod council;
mod import;
mod skills;
mod skills_repo;
mod skills_state;
mod skills_taps;
mod sources;
pub(crate) mod context;
mod context_manage;
mod flags;
mod folders;
mod hooks;
mod mcp;
mod obs;
mod output;
mod plugins;
mod policy;
mod profile;
mod secret;
mod server;
mod styles;
mod sync;
mod update;
mod usage;

pub use commands_json::registry;
pub use output::ErrorKind;
pub use policy::{terminal_only, Needs, Preview, Surface, Tier, ToolPreview};
use output::{CtlError, Envelope, Output};

pub const SCHEMA_VERSION: u32 = 1;
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

pub type Handler = fn(&[String]) -> Result<Output, CtlError>;

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

macro_rules! planned_groups {
    ($($name:ident => $label:literal),* $(,)?) => {$(
        fn $name(_: &[String]) -> Result<Output, CtlError> {
            Err(CtlError::not_implemented(concat!($label, ": not implemented"),
            ))
        }
    )*};
}

planned_groups!(
    server_group => "server",
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
        &["commands"],
        "List all commands with tier, dry-run support and flags (the registry)",
        commands_json::run,
    )
    .no_options(),
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
    cmd(&["profile", "ls"], "List profiles and their servers (--verbose)", profile::ls),
    cmd(
        &["profile", "create"],
        "Create an empty profile (<name>, --force, --dry-run)",
        profile::create,
    ),
    cmd(
        &["profile", "edit"],
        "Rename a profile or change its servers (--name, --servers, --add-server, --remove-server, --dry-run)",
        profile::edit,
    ),
    cmd(
        &["profile", "rm"],
        "Delete a profile and clean the client entries scoped to it (--no-clients, --dry-run)",
        profile::rm,
    ),
    cmd(&["profile"], "Profiles: ls create edit rm inspect", profile::group),
    cmd(&["client", "ls"], "List detected clients and their direct entries", client::ls),
    cmd(
        &["client", "sync"],
        "Sync managed clients (--client <id>, --dry-run, --keep-orphans)",
        client::sync,
    ),
    cmd(
        &["client", "edit"],
        "Point a client at a profile (<client>, --add-profile, --remove-profile, --set-profiles, --force, --dry-run)",
        client_edit::edit,
    ),
    cmd(
        &["client", "import"],
        "Import a client's direct entries into the registry (<client>, --select, --all, --profile, --dry-run)",
        client_edit::import,
    ),
    cmd(
        &["client", "direct", "add"],
        "Give a client one server as its own direct entry (<server>, --client <id>, --force, --dry-run)",
        client_direct::add,
    ),
    cmd(
        &["client", "direct", "rm"],
        "Remove a direct entry Toolport wrote (<server>, --client <id>, --force, --dry-run)",
        client_direct::rm,
    ),
    cmd(
        &["client", "direct", "ls"],
        "List the direct launcher entries and their state (--client <id>)",
        client_direct::ls,
    ),
    cmd(&["client", "direct"], "Direct entries: add rm ls", client_direct::group),
    cmd(&["client"], "Client configs: ls edit import sync direct", client_edit::group),
    cmd(
        &["direct", "run"],
        "Start a server over stdio for a direct client entry (<server id>)",
        client_direct::run,
    ),
    cmd(&["direct"], "Direct launchers: run", client_direct::run_group),
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
    cmd(&["auth"], "Auth health: statusline hook probe login", auth::group).no_options(),
    cmd(&["secret", "set"], "Store a secret read from stdin or --value-env", secret::set),
    cmd(&["secret", "get"], "Check a secret (--reveal prints the value)", secret::get),
    cmd(&["secret", "rm"], "Remove a secret", secret::rm),
    cmd(&["secret"], "Manage server secrets (not implemented)", secret_group),
    cmd(
        &["context", "loads"],
        "Show what a claude session loads, with token cost (--profile, --cwd, --no-lazy, --measured)",
        context::loads,
    ),
    cmd(
        &["context", "measure"],
        "Measure what a folder really loads, in tokens, with Claude Code (--cwd, --without, --bundle, --model, --force, --yes)",
        context::measure,
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
    cmd(
        &["context", "init"],
        "Scaffold the personal layer and save context.json (--home <dir>, --dry-run, --yes)",
        context_manage::init,
    ),
    cmd(
        &["context", "status"],
        "Show layers, profiles, legacy MCP duplicates and the shims file (--home <dir>)",
        context_manage::status,
    ),
    cmd(
        &["context", "client", "add"],
        "Scaffold a path-scoped client layer (<name>, --glob <pattern>, --home <dir>, --dry-run)",
        context_manage::client_add,
    ),
    cmd(
        &["context", "client", "list"],
        "List the context layers with their path globs (--home <dir>)",
        context_manage::client_list,
    ),
    cmd(
        &["context", "client"],
        "Client layers: add list",
        context_manage::client_group,
    ),
    cmd(
        &["context", "profile", "add"],
        "Define a launch profile and generate it (<name>, --no-org, --rules, --servers, --dry-run)",
        context_manage::profile_add,
    ),
    cmd(
        &["context", "profile", "list"],
        "List the configured launch profiles (--home <dir>)",
        context_manage::profile_list,
    ),
    cmd(
        &["context", "profile", "remove"],
        "Drop a launch profile; --purge deletes its directory (<name>, --purge, --dry-run)",
        context_manage::profile_remove,
    ),
    cmd(
        &["context", "profile"],
        "Launch profiles: add list remove",
        context_manage::profile_group,
    ),
    cmd(
        &["context", "disable"],
        "Remove the generated shims; --purge-profiles also the profile directories (--dry-run)",
        context_manage::disable,
    ),
    cmd(
        &["context"],
        "Context: init status client profile disable loads checkpoint-status plan apply sync",
        context::group,
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
        "Self-management server: install [--profile <id>] uninstall doctor tools call <tool> [--args <json> | --args-stdin]",
        mcp::run,
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
        &["sources", "root"],
        "Search folders: ls add rm",
        sources::root_group,
    ),
    cmd(&["sources"], "Sources: ls root", sources::group),
    cmd(
        &["plugins", "ls"],
        "List installed Claude Code plugins with what each brings and costs (--cwd, --refresh)",
        plugins::ls,
    ),
    cmd(
        &["plugins", "show"],
        "Show one plugin: components, hooks, MCP servers, options (<id>, --cwd)",
        plugins::show,
    ),
    cmd(&["plugins"], "Plugins: ls show", plugins::group),
    cmd(
        &["hooks", "ls"],
        "List every hook Claude Code would start in a folder, from files (--cwd, --tool, --event, --owner)",
        hooks::ls,
    ),
    cmd(&["hooks"], "Hooks: ls", hooks::group),
    cmd(
        &["skills", "sync"],
        "Transpile skills to client outputs (--repo, --home, --client, --project, --dry-run)",
        skills::sync,
    ),
    cmd(
        &["skills", "ls"],
        "List skills and rules (--repo <dir>, --home <dir>, --source <id>)",
        skills::ls,
    ),
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
    cmd(&["skills", "tap"], "Taps: add ls remove update", skills_taps::tap_group),
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
        &["skills"],
        "Skills: init add ls lint audit bundle unbundle sync diff status clean uninstall resolve tap search install",
        skills::group,
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
        &["agents"],
        "Agents: add ls lint audit diff status clean uninstall sync",
        agents::group,
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
        &["styles"],
        "Styles: add ls lint diff status sync apply remove clean",
        styles::group,
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
    cmd(
        &["obs", "otel", "enable"],
        "Send Claude Code telemetry to the local receiver (--port <n>, --home <dir>, --dry-run)",
        obs::otel_enable,
    ),
    cmd(
        &["obs", "otel", "disable"],
        "Stop the receiver and remove the telemetry keys Toolport wrote (--home <dir>, --dry-run)",
        obs::otel_disable,
    ),
    cmd(
        &["obs", "otel", "status"],
        "Show the receiver, the Claude settings and the stored OTel events (--home <dir>)",
        obs::otel_status,
    ),
    cmd(&["obs", "otel"], "OTel receiver: enable disable status", obs::otel_group),
    cmd(&["obs"], "Observability: otel enable|disable|status", obs::group),
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
    } else if code == EXIT_OK || envelope.error.as_ref().map(|e| e.kind) == Some(ErrorKind::Unhealthy)
    {
        let _ = writeln!(out, "{}", human.trim_end());
    } else {
        let _ = writeln!(err, "{}", human.trim_end());
    }
    code
}

#[cfg(test)]
mod auth_tests;
#[cfg(all(test, unix))]
mod context_measure_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod agents_golden_tests;
#[cfg(test)]
mod agents_tests;
#[cfg(test)]
mod skills_client_scope_tests;
#[cfg(test)]
mod styles_golden_tests;
#[cfg(test)]
mod styles_tests;
#[cfg(test)]
mod compression_cfg_tests;
#[cfg(test)]
mod compression_golden_tests;
#[cfg(test)]
mod compression_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod usage_pins_tests;

#[cfg(test)]
mod server_tests;
#[cfg(test)]
mod client_direct_tests;

#[cfg(test)]
mod import_tests;

#[cfg(test)]
mod skills_taps_golden_tests;
#[cfg(test)]
mod skills_taps_tests;
#[cfg(test)]
mod skills_tests;

#[cfg(test)]
mod profile_golden_tests;
#[cfg(test)]
mod profile_tests;
#[cfg(test)]
mod skills_collisions_tests;
