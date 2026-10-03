//! Fixtures shared by the stdio sign-in tests: a mock server that prints a consent URL when it is
//! started as `<command> auth`, with its behaviour picked by the `MOCK_MODE` environment setting.

use std::path::Path;

use serde_json::{json, Value};

use crate::registry::ServerEntry;

pub const CONSENT_URL: &str = "https://auth.example.invalid/consent?state=abc123";
pub const LEAKY: &str = "FAKE-LEAK-SECRET-9f2c41";

#[cfg(unix)]
const SCRIPT: &str = r#"#!/bin/sh
for last; do :; done
[ "$last" = "auth" ] || exit 64
[ -z "$MOCK_PID" ] || echo $$ > "$MOCK_PID"
case "$MOCK_MODE" in
  consent)
    if [ -e "$MOCK_DONE" ]; then echo "already signed in" >&2; exit 0; fi
    echo "Open https://auth.example.invalid/consent?state=abc123 to sign in" >&2
    [ -z "$MOCK_PRINTED" ] || : > "$MOCK_PRINTED"
    while [ ! -e "$MOCK_DONE" ]; do sleep 0.05; done
    exit "${MOCK_EXIT:-0}" ;;
  signed_in)
    echo "already signed in" >&2
    exit 0 ;;
  expired)
    echo "token expired, sign in again at https://auth.example.invalid/consent?state=abc123" >&2
    sleep 30 & echo $! > "$MOCK_CHILD"; wait ;;
  revoked)
    echo "access was revoked for this account" >&2
    exit 1 ;;
  leak)
    echo "using key $MOCK_SECRET" >&2
    echo "args: $*" >&2
    echo "vault=${TOOLPORT_SECRET_KEY:-unset}" >&2
    echo "Open https://auth.example.invalid/consent?state=abc123" >&2
    [ -z "$MOCK_PRINTED" ] || : > "$MOCK_PRINTED"
    while [ ! -e "$MOCK_DONE" ]; do sleep 0.05; done
    exit 0 ;;
  bound_url)
    echo "Open https://auth.example.invalid/consent?token=$2" >&2
    sleep 30 ;;
  crash)
    exit 3 ;;
  silent)
    sleep 30 ;;
esac
"#;

#[cfg(unix)]
pub fn install_script(dir: &Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("acme-auth-mock");
    crate::plus::testutil::exec::write_executable(&path, SCRIPT);
    path
}

pub fn env_var(key: &str, value: &str, secret: bool) -> Value {
    json!({"key": key, "value": if secret { Value::Null } else { json!(value) }, "secret": secret})
}

pub fn stdio_json(
    id: &str,
    command: &Path,
    mode: &str,
    extra_env: Vec<Value>,
    hinted: bool,
) -> Value {
    let mut env = vec![env_var("MOCK_MODE", mode, false)];
    env.extend(extra_env);
    let mut server = json!({
        "id": id,
        "name": id,
        "transport": "stdio",
        "command": command.to_string_lossy(),
        "args": [],
        "env": env,
    });
    if hinted {
        server["plus"] = json!({"authProbe": {"kind": "stdio"}});
    }
    server
}

pub fn server(value: Value) -> ServerEntry {
    serde_json::from_value(value).unwrap()
}

pub fn write_registry(servers: Vec<Value>, enabled: &[&str]) {
    let registry = json!({
        "version": 1,
        "servers": servers,
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": enabled}],
        "activeProfileId": "default",
    });
    let path = crate::registry::conduit_dir()
        .unwrap()
        .join("registry.json");
    std::fs::write(path, serde_json::to_string_pretty(&registry).unwrap()).unwrap();
}

#[cfg(unix)]
pub fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
pub fn wait_gone(pid_file: &Path) -> bool {
    let pid = std::fs::read_to_string(pid_file).unwrap_or_default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while alive(&pid) {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    true
}

#[cfg(unix)]
pub struct World {
    pub dir: std::path::PathBuf,
    pub script: std::path::PathBuf,
}

#[cfg(unix)]
impl World {
    pub fn path(&self, name: &str) -> std::path::PathBuf {
        self.dir.join(name)
    }

    pub fn done(&self) -> std::path::PathBuf {
        self.path("done")
    }

    pub fn printed(&self) -> std::path::PathBuf {
        self.path("printed")
    }
}

#[cfg(unix)]
pub fn with_world(tag: &str, test: impl FnOnce(&World)) {
    crate::secrets::tests::with_isolated_vault(|| {
        let dir = std::env::temp_dir().join(format!("auth-mock-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let script = install_script(&dir);
        test(&World {
            dir: dir.clone(),
            script,
        });
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[cfg(unix)]
pub fn mock(world: &World, id: &str, mode: &str) -> Value {
    let paths = [
        ("MOCK_PID", "pid"),
        ("MOCK_CHILD", "child"),
        ("MOCK_DONE", "done"),
        ("MOCK_PRINTED", "printed"),
    ];
    let extra = paths
        .iter()
        .map(|(key, name)| env_var(key, &world.path(name).to_string_lossy(), false))
        .collect();
    stdio_json(id, &world.script, mode, extra, true)
}
