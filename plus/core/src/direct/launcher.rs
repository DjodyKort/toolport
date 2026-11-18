//! `toolportctl direct run <server>`: the stdio launcher a direct client entry points at. It asks
//! the host for the server's command with its secrets read from the vault now, then replaces
//! itself with the server (unix), so stdin, stdout and the exit status are the server's own.
//! Nothing here writes to stdout: it is the client's protocol channel.

use std::process::Command;

use toolport_plus_api::{Host, LaunchCommand, ServerRecord};

const CONTAINER_RUNTIMES: [&str; 3] = ["docker", "podman", "nerdctl"];

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

/// Why a server cannot be started without the gateway, or `None` when it can. A remote server
/// has no process of ours to start; its direct entry holds the address only.
pub fn refusal(server: &ServerRecord) -> Option<String> {
    if server.extra.contains_key("clientCredentials") {
        return Some(
            "its sign-in is managed by Toolport (OAuth) and only the gateway can refresh it".into(),
        );
    }
    let Some(command) = server.text("command") else {
        return Some("it is a remote server and only the gateway can reach it".into());
    };
    let stdio = server
        .text("transport")
        .unwrap_or("stdio")
        .eq_ignore_ascii_case("stdio");
    if !stdio {
        return Some("it is a remote server and only the gateway can reach it".into());
    }
    if server
        .text("cwd")
        .is_some_and(|cwd| cwd.contains("${ROOT}"))
    {
        return Some(
            "its working directory follows the client's project root (${ROOT}), which only the gateway knows"
                .into(),
        );
    }
    let needs_env_in_container = CONTAINER_RUNTIMES
        .contains(&super::basename_stem(command).as_str())
        && server.env().is_ok_and(|env| !env.is_empty());
    if needs_env_in_container {
        return Some(
            "it runs in a container and the gateway passes its environment into it with -e flags, which a launcher entry cannot"
                .into(),
        );
    }
    None
}

pub fn is_run(positional: &[String]) -> bool {
    matches!(positional, [a, b, _] if a == "direct" && b == "run")
}

pub fn start(host: &dyn Host, key: &str) -> Result<LaunchCommand, Failure> {
    let servers = host
        .list_servers()
        .map_err(|error| Failure::new(format!("registry is not readable: {error}")))?;
    let key = key.trim();
    let server = servers
        .iter()
        .find(|server| server.id == key)
        .or_else(|| servers.iter().find(|s| s.name.eq_ignore_ascii_case(key)))
        .ok_or_else(|| {
            Failure::new(format!(
                "server '{key}' is not in the registry; remove the client entry with \
                 `toolportctl client direct rm {key} --client <id>`"
            ))
        })?;
    if let Some(reason) = refusal(server) {
        return Err(Failure::new(format!(
            "{} cannot run directly: {reason}",
            server.name
        )));
    }
    host.prepare_launch(&server.id).map_err(Failure::new)
}

pub fn command_of(launch: &LaunchCommand) -> Command {
    let mut command = Command::new(&launch.program);
    command
        .env_clear()
        .args(&launch.args)
        .envs(launch.env.iter().map(|(k, v)| (k, v)));
    if let Some(dir) = &launch.cwd {
        command.current_dir(dir);
    }
    command
}

#[cfg(unix)]
fn replace_process(mut command: Command) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    command.exec()
}

#[cfg(not(unix))]
fn replace_process(mut command: Command) -> std::io::Error {
    match command.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => error,
    }
}

/// Never returns on success: the process becomes the server. A failure to start is the only way
/// back.
pub fn run(host: &dyn Host, key: &str) -> Failure {
    let launch = match start(host, key) {
        Ok(launch) => launch,
        Err(failure) => return failure,
    };
    let error = replace_process(command_of(&launch));
    let exit = if error.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    };
    Failure {
        message: format!("failed to start '{}': {error}", launch.program),
        exit,
    }
}

/// The entry point of the command: reports a failure on stderr and returns the exit code.
pub fn main(host: &dyn Host, positional: &[String]) -> i32 {
    let failure = run(host, &positional[2]);
    eprintln!("toolportctl: {}", failure.message);
    failure.exit
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use toolport_plus_api::{convert, MemoryClients, MemoryHost, MemoryRegistry};

    use super::*;

    fn server(value: serde_json::Value) -> ServerRecord {
        convert(&value).unwrap()
    }

    fn stdio(id: &str, name: &str) -> ServerRecord {
        server(json!({"id": id, "name": name, "transport": "stdio", "command": "run-it"}))
    }

    fn host_with(servers: Vec<ServerRecord>) -> MemoryHost {
        MemoryHost {
            registry: MemoryRegistry::with_servers(servers),
            clients: MemoryClients::with_client("claude-code", "Claude Code"),
            ..Default::default()
        }
    }

    fn launch(program: &str, args: &[&str]) -> LaunchCommand {
        LaunchCommand {
            program: program.into(),
            args: args.iter().map(|arg| (*arg).into()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn test01_only_the_gateway_can_start_oauth_remote_root_and_container_env_servers() {
        let oauth = server(json!({"id": "a", "name": "a", "transport": "stdio",
            "command": "x", "clientCredentials": {"clientId": "c"}}));
        let remote = server(json!({"id": "b", "name": "b", "transport": "http",
            "url": "https://x.invalid/mcp"}));
        let rooted = server(json!({"id": "c", "name": "c", "transport": "stdio",
            "command": "x", "cwd": "${ROOT}/sub"}));
        let docker = server(json!({"id": "d", "name": "d", "transport": "stdio",
            "command": "/usr/bin/docker", "args": ["run", "-i", "img"],
            "env": [{"key": "K", "secret": true}]}));
        for (record, needle) in [
            (&oauth, "OAuth"),
            (&remote, "remote"),
            (&rooted, "${ROOT}"),
            (&docker, "container"),
        ] {
            let reason = refusal(record).unwrap_or_else(|| panic!("{needle} not refused"));
            assert!(reason.contains(needle), "{reason}");
        }
    }

    #[test]
    fn test02_a_container_server_without_environment_and_a_plain_server_are_not_refused() {
        let docker = server(json!({"id": "d", "name": "d", "transport": "stdio",
            "command": "docker", "args": ["run", "-i", "img"]}));
        assert_eq!(refusal(&docker), None);
        assert_eq!(refusal(&stdio("a", "alpha")), None);
    }

    #[test]
    fn test03_an_unknown_server_names_the_cleanup_command() {
        let host = host_with(vec![stdio("srv-a", "alpha")]);
        let failure = start(&host, "gone").unwrap_err();
        assert_eq!(failure.exit, 1);
        assert!(failure.message.contains("client direct rm gone"));
    }

    #[test]
    fn test04_a_refused_server_does_not_reach_the_host() {
        let remote = server(json!({"id": "b", "name": "beta", "transport": "http",
            "url": "https://x.invalid/mcp"}));
        let host = host_with(vec![remote]);
        let failure = start(&host, "b").unwrap_err();
        assert!(failure.message.starts_with("beta cannot run directly"));
    }

    #[test]
    fn test05_the_host_error_is_the_failure_and_the_launch_is_found_by_id_or_name() {
        let mut host = host_with(vec![stdio("srv-a", "alpha")]);
        host.clients
            .launches
            .insert("srv-a".into(), Err("missing secret 'K'".into()));
        assert_eq!(
            start(&host, "alpha").unwrap_err().message,
            "missing secret 'K'"
        );
        host.clients
            .launches
            .insert("srv-a".into(), Ok(launch("/bin/true", &[])));
        assert_eq!(start(&host, "srv-a").unwrap().program, "/bin/true");
        assert_eq!(start(&host, "ALPHA").unwrap().program, "/bin/true");
    }

    #[test]
    fn test06_the_launch_debug_output_names_env_keys_and_no_value() {
        let mut command = launch("/bin/true", &[]);
        command.env.push(("K".into(), "s3cr3t-value".into()));
        let shown = format!("{command:?}");
        assert!(shown.contains("\"K\"") && !shown.contains("s3cr3t-value"));
    }

    #[cfg(unix)]
    #[test]
    fn test07_the_server_runs_with_exactly_the_given_environment_arguments_and_directory() {
        let dir = std::env::temp_dir();
        let mut command = launch(
            "/bin/sh",
            &[
                "-c",
                "printf '%s|%s|%s|%s' \"$FAKE_KEY\" \"${HOME-unset}\" \"$1\" \"$(pwd -P)\"",
                "sh",
                "arg-one",
            ],
        );
        command.env.push(("FAKE_KEY".into(), "fake-value".into()));
        command.cwd = Some(dir.to_string_lossy().into_owned());
        let output = command_of(&command).output().unwrap();
        let expected = format!(
            "fake-value|unset|arg-one|{}",
            dir.canonicalize().unwrap().display()
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }

    #[cfg(unix)]
    #[test]
    fn test08_a_server_that_cannot_be_found_is_a_failure_not_a_panic() {
        let error = command_of(&launch("/nonexistent/server", &[]))
            .output()
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn test09_only_direct_run_with_a_server_is_the_launcher() {
        let argv = |parts: &[&str]| parts.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        assert!(is_run(&argv(&["direct", "run", "srv"])));
        assert!(!is_run(&argv(&["direct", "run"])));
        assert!(!is_run(&argv(&["client", "direct", "add"])));
    }
}
