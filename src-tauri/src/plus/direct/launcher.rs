//! `toolportctl direct run <server>`: the stdio launcher a direct client entry points at. It
//! resolves the server's env and secrets from the secret store at start, builds the command with
//! the same screening the gateway spawns with, and replaces itself with the server so stdin,
//! stdout and the exit status are the server's own. Nothing here writes to stdout.

use crate::downstream;
use crate::plus::{registry_ro, servers};
use crate::registry::ServerEntry;
use std::collections::HashSet;
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    pub exit: i32,
}

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit: 1,
        }
    }
}

/// Why a server cannot be launched without the gateway, or `None` when it can.
pub fn refusal(server: &ServerEntry) -> Option<String> {
    if crate::clients::is_gateway_server(server) {
        return Some("it is the Toolport gateway itself".into());
    }
    if server.command.is_none() || !server.transport.eq_ignore_ascii_case("stdio") {
        return Some(format!(
            "it is a remote ({}) server and only the gateway can reach it",
            server.transport
        ));
    }
    if server.client_credentials.is_some() {
        return Some(
            "its sign-in is managed by Toolport (OAuth) and only the gateway can refresh it".into(),
        );
    }
    if let Some(cwd) = server.cwd.as_deref() {
        if cwd.contains("${ROOT}") {
            return Some(
                "its working directory follows the client's project root (${ROOT}), which only \
                 the gateway knows"
                    .into(),
            );
        }
    }
    None
}

/// The command a direct run executes, from the server's resolved args and env.
pub fn prepare(
    server: &ServerEntry,
    args: &[String],
    env: &[(String, String)],
) -> Result<Command, String> {
    if let Some(reason) = refusal(server) {
        return Err(format!("{} cannot run directly: {reason}", server.name));
    }
    let command = server.command.as_deref().unwrap_or_default();
    let (command, args) = downstream::normalize_invocation(command, args);
    downstream::screen_spawn_command(&command, &args)?;
    downstream::screen_spawn_env(env)?;
    let args = downstream::inject_container_env(&command, &args, env);
    let mut cmd = Command::new(downstream::resolve_command(&command));
    crate::hostenv::strip_bundled_env(&mut cmd);
    cmd.args(&args).envs(env.iter().cloned());
    let configured: HashSet<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    downstream::strip_gateway_control_env(&mut cmd, &configured);
    if let Some(dir) = server
        .cwd
        .as_deref()
        .and_then(|cwd| downstream::resolve_root_token(cwd, None))
    {
        cmd.current_dir(downstream::validate_cwd(&dir)?);
    }
    #[cfg(not(windows))]
    cmd.env("PATH", downstream::augmented_path());
    Ok(cmd)
}

fn resolve(server: &ServerEntry) -> Result<(Vec<String>, Vec<(String, String)>), String> {
    let env = crate::server_runtime::environment_for_probe_with(
        server,
        crate::secrets::get_secret_result,
    )?;
    let resolved = crate::launch_inputs::resolve_args(server)?;
    Ok((resolved.args, env))
}

fn redact(server: &ServerEntry, env: &[(String, String)], message: String) -> String {
    crate::launch_inputs::redact_env_secrets(server, env, message)
}

pub(crate) fn start(server: &ServerEntry) -> Result<Command, Failure> {
    let (args, env) = resolve(server).map_err(|error| {
        let hint = if error.starts_with("missing secret") {
            format!(
                "{error} (set it with `toolportctl secret set {} <KEY>`)",
                server.id
            )
        } else {
            error
        };
        Failure::new(hint)
    })?;
    prepare(server, &args, &env).map_err(|error| Failure::new(redact(server, &env, error)))
}

#[cfg(unix)]
fn run_command(mut cmd: Command) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    cmd.exec()
}

#[cfg(not(unix))]
fn run_command(mut cmd: Command) -> std::io::Error {
    match cmd.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => error,
    }
}

/// Never returns on success. A failure to start is the only way back.
pub fn run(key: &str) -> Failure {
    let reg = match registry_ro::read() {
        Ok(reg) => reg,
        Err(error) => return Failure::new(error),
    };
    let Some(server) = servers::find(&reg, key.trim()).cloned() else {
        return Failure::new(format!(
            "server '{key}' is not in the registry; remove the client entry with \
             `toolportctl client direct rm {key} --client <id>`"
        ));
    };
    let cmd = match start(&server) {
        Ok(cmd) => cmd,
        Err(failure) => return failure,
    };
    let program = cmd.get_program().to_string_lossy().into_owned();
    let error = run_command(cmd);
    let exit = if error.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    };
    Failure {
        message: format!("failed to start '{program}': {error}"),
        exit,
    }
}
