//! What the runner needs from the outside world, behind one trait so tests use a fake MCP
//! server and a fake vault. `RealHost` is the production implementation.

use serde_json::Value;
use std::sync::Arc;

pub trait Host: Send + Sync {
    fn call_tool(&self, server: &str, tool: &str, args: Value) -> Result<Value, String>;
    fn run_routine(&self, routine_id: Option<&str>, script: Option<&str>, args: Value) -> Result<Value, String>;
    fn set_secret(&self, server: &str, key: &str, value: &str) -> Result<(), String>;
    fn secret_is_set(&self, server: &str, key: &str) -> bool;
    fn restart_server(&self, server: &str) -> Result<String, String>;
    fn claude_program(&self) -> String;
    fn spawn_runner(&self, run_id: &str) -> Result<(), String>;
}

pub struct RealHost;

fn is_launch_secret(server: &str, key: &str) -> bool {
    crate::plus::registry_ro::read_opt().is_some_and(|reg| reg.servers.iter().find(|s| s.id == server).and_then(|s| s.launch.as_ref()).is_some_and(|l| l.inputs.iter().any(|i| i.key == key && i.secret)))
}

fn ctl_program() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("TOOLPORT_CTL_BIN") {
        return path.into();
    }
    let name = if cfg!(windows) { "toolportctl.exe" } else { "toolportctl" };
    std::env::current_exe().ok().map(|exe| exe.with_file_name(name)).filter(|p| p.exists()).unwrap_or_else(|| name.into())
}

impl Host for RealHost {
    fn call_tool(&self, server: &str, tool: &str, args: Value) -> Result<Value, String> {
        crate::playground::call_tool(server, tool, args)
    }

    fn run_routine(&self, routine_id: Option<&str>, script: Option<&str>, args: Value) -> Result<Value, String> {
        let (source, limits) = match (routine_id, script) {
            (Some(id), _) => {
                let def = crate::routines::get(id)?.ok_or_else(|| format!("no routine {id:?}"))?;
                (def.source().to_string(), def.limits().effective())
            }
            (None, Some(script)) => (script.to_string(), crate::codemode::Limits::default()),
            (None, None) => return Err("routine step has neither routineId nor script".into()),
        };
        let call: crate::codemode::CallBinding = Arc::new(|name: &str, args: Value| {
            let Some((server, tool)) = crate::codemode::split_exposed_name(name) else {
                return serde_json::json!({"error": format!("{name:?} is not server__tool")});
            };
            crate::playground::call_tool(server, tool, args).unwrap_or_else(|e| serde_json::json!({"isError": true, "error": e}))
        });
        let outcome = crate::codemode::run_script(&source, args, call, None, limits, &[]);
        match outcome.error {
            Some(error) => Err(error),
            None => Ok(outcome.value),
        }
    }

    fn set_secret(&self, server: &str, key: &str, value: &str) -> Result<(), String> {
        if is_launch_secret(server, key) {
            crate::registry_controller::set_launch_secret(server, key, value)
        } else {
            crate::registry_controller::set_server_secret(server, key, value)
        }
        .map(|_| ())
    }

    fn secret_is_set(&self, server: &str, key: &str) -> bool {
        crate::secrets::get_secret_result(server, key).ok().flatten().is_some()
    }

    fn restart_server(&self, server: &str) -> Result<String, String> {
        let registry = crate::registry::load()?;
        if !registry.servers.iter().any(|s| s.id == server) {
            return Err(format!("server {server:?} is not in the registry"));
        }
        crate::registry::update(|r| {
            r.secrets_generation = r.secrets_generation.wrapping_add(1);
            Ok(())
        })?;
        Ok(format!("asked the gateway to respawn {server}"))
    }

    fn claude_program(&self) -> String {
        std::env::var("TOOLPORT_CLAUDE_BIN").unwrap_or_else(|_| "claude".into())
    }

    fn spawn_runner(&self, run_id: &str) -> Result<(), String> {
        let mut cmd = std::process::Command::new(ctl_program());
        cmd.args(["task", "__run", run_id]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().map_err(|e| format!("cannot start the task runner: {e}"))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}
