//! Runs `git` with a timeout and no terminal prompt, for the tap clones.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NOT_FOUND_PREFIX: &str = "could not start (not found)";
const SPAWN_RETRIES: u32 = 3;
const SPAWN_RETRY_DELAY: Duration = Duration::from_millis(20);
const FIRST_POLL: Duration = Duration::from_millis(1);
const MAX_POLL: Duration = Duration::from_millis(10);

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

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
}

pub fn run_command(cmd: Command, timeout: Duration) -> Result<CmdOutput, String> {
    run_command_input(cmd, None, timeout)
}

/// [`run_command`] with `input` written to the child's stdin (then closed), for values that must
/// not appear in an argument list.
pub fn run_command_input(
    mut cmd: Command,
    input: Option<&str>,
    timeout: Duration,
) -> Result<CmdOutput, String> {
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = spawn_retrying(&mut cmd).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("{NOT_FOUND_PREFIX}: {e}")
        } else {
            format!("could not start: {e}")
        }
    })?;
    let out_thread = drain(child.stdout.take());
    let err_thread = drain(child.stderr.take());
    let feed = input.zip(child.stdin.take()).map(|(text, mut pipe)| {
        let text = text.to_string();
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = pipe.write_all(text.as_bytes());
        })
    });
    let started = Instant::now();
    let mut nap = FIRST_POLL;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => {
                std::thread::sleep(nap);
                nap = (nap * 2).min(MAX_POLL);
            }
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    };
    if let Some(feed) = feed {
        let _ = feed.join();
    }
    Ok(CmdOutput {
        code: status.code().unwrap_or(-1),
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
    })
}

/// True when `error` came from [`run_command`] failing because the program does not exist.
pub fn is_not_found(error: &str) -> bool {
    error.starts_with(NOT_FOUND_PREFIX)
}

pub fn git_command(dir: Option<&Path>, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    if let Some(dir) = dir {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

/// Runs git and returns its untrimmed stdout; a non-zero exit becomes `git <sub> failed: <stderr>`.
pub fn git_stdout(dir: Option<&Path>, args: &[&str], timeout: Duration) -> Result<String, String> {
    let sub = args.first().copied().unwrap_or("");
    let out =
        run_command(git_command(dir, args), timeout).map_err(|e| format!("git {sub}: {e}"))?;
    if out.ok() {
        Ok(out.stdout)
    } else {
        Err(format!("git {sub} failed: {}", out.stderr.trim()))
    }
}
