use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NOT_FOUND_PREFIX: &str = "could not start (not found)";
const SPAWN_RETRIES: u32 = 3;
const SPAWN_RETRY_DELAY: Duration = Duration::from_millis(20);

/// Spawn, retrying a few times when the kernel answers `ETXTBSY`.
///
/// A script written moments ago can still be open for writing in a process that
/// forked concurrently, until that child reaches its own `exec`; the kernel
/// refuses to execute the file for that long.
fn spawn_retrying(cmd: &mut Command) -> std::io::Result<Child> {
    let mut retries = 0;
    loop {
        match cmd.spawn() {
            Err(e) if is_text_busy(&e) && retries < SPAWN_RETRIES => {
                retries += 1;
                std::thread::sleep(SPAWN_RETRY_DELAY);
            }
            other => return other,
        }
    }
}

fn is_text_busy(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::ExecutableFileBusy
}

/// True when `error` came from [`run_command`] failing because the program does
/// not exist, as opposed to any other reason it could not start.
pub fn is_not_found(error: &str) -> bool {
    error.starts_with(NOT_FOUND_PREFIX)
}

#[derive(Debug, Clone, Default)]
pub struct CmdOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOutput {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    pub fn first_error_line(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        text.lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .chars()
            .take(200)
            .collect()
    }
}

pub trait GitRunner {
    fn git(&self, repo: &Path, args: &[&str], timeout: Duration) -> Result<CmdOutput, String>;
}

pub trait ShellRunner {
    fn run(&self, command: &str, cwd: &Path, timeout: Duration) -> Result<CmdOutput, String>;
}

pub fn run_command(mut cmd: Command, timeout: Duration) -> Result<CmdOutput, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_retrying(&mut cmd).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("{NOT_FOUND_PREFIX}: {e}")
        } else {
            format!("could not start: {e}")
        }
    })?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    };
    Ok(CmdOutput {
        code: status.code().unwrap_or(-1),
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
    })
}

pub struct SystemGit;

impl GitRunner for SystemGit {
    fn git(&self, repo: &Path, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(repo)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0");
        run_command(cmd, timeout).map_err(|e| {
            if is_not_found(&e) {
                "git not found on PATH".to_string()
            } else {
                e
            }
        })
    }
}

pub struct SystemShell;

impl ShellRunner for SystemShell {
    fn run(&self, command: &str, cwd: &Path, timeout: Duration) -> Result<CmdOutput, String> {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(command);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);
            c
        };
        cmd.current_dir(cwd);
        run_command(cmd, timeout)
    }
}
