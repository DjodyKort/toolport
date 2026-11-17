//! The real process side of a task run: children in their own process group, killed as a group,
//! and the detached runner child the scheduler and `task run` start.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use toolport_plus_api::{ProcessOutcome, ProcessSpec};

const CHILD_ENV_DROP: [&str; 3] = [
    "TOOLPORT_SECRET_KEY",
    "CONDUIT_SECRET_KEY",
    "TOOLPORT_HTTP_TOKEN",
];
const READ_CAP: usize = 64 * 1024;

fn poll() -> Duration {
    Duration::from_millis(
        std::env::var("TOOLPORT_TASK_POLL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100),
    )
}

pub fn kill_group(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

fn read_capped(mut from: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = from.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut b = sink
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if b.len() < READ_CAP {
                b.extend_from_slice(&chunk[..n]);
            }
        }
    });
    buf
}

pub fn run_process(
    spec: &ProcessSpec,
    cancelled: &dyn Fn() -> bool,
    started: &dyn Fn(u32),
) -> ProcessOutcome {
    let program = if spec.program == CTL_NAME {
        ctl_program()
    } else {
        PathBuf::from(&spec.program)
    };
    let mut cmd = Command::new(program);
    cmd.args(&spec.args);
    for name in CHILD_ENV_DROP {
        cmd.env_remove(name);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ProcessOutcome::SpawnFailed(e.to_string()),
    };
    let pid = child.id();
    started(pid);
    let out = child.stdout.take().map(read_capped);
    let err = child.stderr.take().map(read_capped);
    let mut last_check = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if last_check.elapsed() >= poll() {
            last_check = Instant::now();
            if cancelled() {
                kill_group(pid);
                let _ = child.kill();
                let _ = child.wait();
                return ProcessOutcome::Cancelled;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = |b: Option<Arc<Mutex<Vec<u8>>>>| {
        b.map(|b| {
            String::from_utf8_lossy(&b.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
                .into_owned()
        })
        .unwrap_or_default()
    };
    std::thread::sleep(Duration::from_millis(30));
    let output = format!("{}{}", text(out), text(err));
    match status {
        Some(s) => ProcessOutcome::Exited {
            code: s.code(),
            output,
        },
        None => ProcessOutcome::SpawnFailed("could not wait for the process".into()),
    }
}

/// An exec step that names this program runs the bundled ctl, the one that starts the runner.
pub const CTL_NAME: &str = "toolportctl";

fn ctl_program() -> PathBuf {
    if let Some(path) = std::env::var_os("TOOLPORT_CTL_BIN") {
        return path.into();
    }
    let name = if cfg!(windows) {
        "toolportctl.exe"
    } else {
        CTL_NAME
    };
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name(name))
        .filter(|p| p.exists())
        .unwrap_or_else(|| name.into())
}

/// Starts `toolportctl task __run <run-id>` detached and returns at once.
pub fn spawn_runner(run_id: &str) -> Result<(), String> {
    let mut cmd = Command::new(ctl_program());
    cmd.args(["task", "__run", run_id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start the task runner: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn an_exec_step_named_toolportctl_runs_the_bundled_ctl() {
        let dir = std::env::temp_dir().join(format!("plus-proc-ctl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctl = dir.join("fake-ctl");
        std::fs::write(&ctl, "#!/bin/sh\necho \"ctl $@\"\n").unwrap();
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("TOOLPORT_CTL_BIN", &ctl);
        let outcome = run_process(
            &ProcessSpec {
                program: CTL_NAME.into(),
                args: vec!["skills".into(), "sync".into()],
            },
            &|| false,
            &|_| {},
        );
        std::env::remove_var("TOOLPORT_CTL_BIN");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            outcome,
            ProcessOutcome::Exited {
                code: Some(0),
                output: "ctl skills sync\n".into()
            }
        );
    }
}
