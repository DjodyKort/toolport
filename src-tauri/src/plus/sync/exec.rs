use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub trait Exec {
    fn git(&self, dir: Option<&Path>, args: &[&str]) -> Result<String, String>;

    fn shell(&self, dir: &Path, command: &str) -> Result<(), String>;
}

pub struct SystemExec;

const GIT_TIMEOUT: Duration = Duration::from_secs(60);
const SHELL_TIMEOUT: Duration = Duration::from_secs(120);

fn run_bounded(mut cmd: Command, timeout: Duration) -> Result<(bool, String, String), String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    Ok((
        status.success(),
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    ))
}

impl Exec for SystemExec {
    fn git(&self, dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
        let mut cmd = Command::new("git");
        if let Some(dir) = dir {
            cmd.arg("-C").arg(dir);
        }
        cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");
        let (ok, stdout, stderr) = run_bounded(cmd, GIT_TIMEOUT)
            .map_err(|e| format!("git {}: {e}", args.first().copied().unwrap_or("")))?;
        if ok {
            Ok(stdout)
        } else {
            Err(format!(
                "git {} failed: {}",
                args.first().copied().unwrap_or(""),
                stderr.trim()
            ))
        }
    }

    fn shell(&self, dir: &Path, command: &str) -> Result<(), String> {
        #[cfg(windows)]
        let (program, flag) = ("cmd", "/C");
        #[cfg(not(windows))]
        let (program, flag) = ("sh", "-c");
        let mut cmd = Command::new(program);
        cmd.arg(flag).arg(command).current_dir(dir);
        let (ok, _, stderr) = run_bounded(cmd, SHELL_TIMEOUT)?;
        if ok {
            Ok(())
        } else {
            Err(stderr.trim().chars().take(200).collect())
        }
    }
}
