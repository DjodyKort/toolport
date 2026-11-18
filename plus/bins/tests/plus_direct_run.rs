//! `toolportctl direct run <server>` end to end: the launcher a direct client entry points at, run
//! the way a client runs it (argv and env from the client file, its own pipes), against fake
//! servers in a synthetic world. The secret is a fixture value; nothing outside the scratch tree is
//! read.

#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

mod common;
use common::world::World;

const VAULTED: &str = "FAKE-VAULT-VALUE-direct-run-5c1e";

const FAKE_SERVER: &str = "#!/bin/sh\nread line\nprintf 'got=%s|arg=%s|plain=%s|token=%s|ambient=%s|ctl=%s|cwd=%s\\n' \"$line\" \"$2\" \"$PLAIN\" \"$API_TOKEN\" \"${AMBIENT_X-unset}\" \"${TOOLPORT_SECRET_KEY-unset}\" \"$(pwd -P)\"\nprintf 'diagnostic\\n' >&2\nexit 7\n";

fn world_with_server(tag: &str) -> World {
    let world = World::new(tag);
    let script = world.base.join("fake-server");
    std::fs::write(&script, FAKE_SERVER).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut registry = world.registry();
    registry["servers"].as_array_mut().unwrap().push(json!({
        "id": "srv-gamma", "name": "gamma", "transport": "stdio",
        "command": script.to_string_lossy(), "args": ["--mode", "direct"],
        "cwd": world.base.to_string_lossy(), "inheritEnv": false,
        "env": [{"key": "PLAIN", "value": "plain-1"}, {"key": "API_TOKEN", "secret": true}]
    }));
    world.write_registry(&registry);
    common::world::write_json(&world.home.join(".claude.json"), &json!({"mcpServers": {}}));
    world
}

fn ctl(world: &World, argv: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .args(argv)
        .env_clear()
        .current_dir(&world.home)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in world.env() {
        command.env(key, value);
    }
    command.env("AMBIENT_X", "client-credential");
    let mut child = command.spawn().unwrap();
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn client_file(world: &World) -> Value {
    common::world::read_json(&world.home.join(".claude.json"))
}

#[test]
fn test01_the_entry_a_client_runs_starts_the_server_with_its_vault_secret_and_pipes() {
    let world = world_with_server("direct-run-journey");
    assert!(ctl(
        &world,
        &["secret", "set", "srv-gamma", "API_TOKEN"],
        Some(VAULTED)
    )
    .status
    .success());
    let added = ctl(
        &world,
        &[
            "client",
            "direct",
            "add",
            "gamma",
            "--client",
            "claude-code",
        ],
        None,
    );
    assert!(added.status.success(), "{}", text(&added.stderr));

    let file = client_file(&world);
    let entry = &file["mcpServers"]["gamma"];
    assert!(!file.to_string().contains(VAULTED));
    let argv: Vec<String> = entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    assert_eq!(argv, ["direct", "run", "srv-gamma"]);

    let mut client = Command::new(entry["command"].as_str().unwrap());
    client
        .args(&argv)
        .env_clear()
        .current_dir(&world.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in world.env() {
        client.env(key, value);
    }
    client.env("AMBIENT_X", "client-credential");
    for (key, value) in entry["env"].as_object().into_iter().flatten() {
        client.env(key, value.as_str().unwrap());
    }
    let mut child = client.spawn().unwrap();
    writeln!(child.stdin.take().unwrap(), "hello").unwrap();
    let output = child.wait_with_output().unwrap();

    let base = world.base.canonicalize().unwrap();
    assert_eq!(
        text(&output.stdout),
        format!(
            "got=hello|arg=direct|plain=plain-1|token={VAULTED}|ambient=unset|ctl=unset|cwd={}\n",
            base.display()
        )
    );
    assert_eq!(text(&output.stderr), "diagnostic\n");
    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn test02_a_missing_secret_stops_the_launcher_on_stderr_and_leaves_stdout_to_the_client() {
    let world = world_with_server("direct-run-missing");
    let output = ctl(&world, &["direct", "run", "srv-gamma"], None);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("missing secret 'API_TOKEN'")
            && stderr.contains("secret set srv-gamma API_TOKEN"),
        "{stderr}"
    );
}

#[test]
fn test03_a_remote_or_unknown_server_is_refused_before_anything_starts() {
    let world = world_with_server("direct-run-refused");
    let remote = ctl(&world, &["direct", "run", "srv-beta"], None);
    assert_eq!(remote.status.code(), Some(1));
    assert!(remote.stdout.is_empty());
    assert!(text(&remote.stderr).contains("beta cannot run directly"));
    let unknown = ctl(&world, &["direct", "run", "nothing"], None);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(text(&unknown.stderr).contains("is not in the registry"));
}

#[test]
fn test04_a_command_that_does_not_exist_exits_127() {
    let world = world_with_server("direct-run-notfound");
    let mut registry = world.registry();
    registry["servers"][2]["command"] = json!("/nonexistent/plus-server");
    registry["servers"][2]["env"] = json!([]);
    world.write_registry(&registry);
    let output = ctl(&world, &["direct", "run", "srv-gamma"], None);
    assert_eq!(output.status.code(), Some(127), "{}", text(&output.stderr));
    assert!(output.stdout.is_empty());
}
