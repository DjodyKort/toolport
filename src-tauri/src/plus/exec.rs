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

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
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
    let out_thread = drain(child.stdout.take());
    let err_thread = drain(child.stderr.take());
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
    Ok(CmdOutput {
        code: status.code().unwrap_or(-1),
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
    })
}

pub fn git_command(dir: Option<&Path>, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    if let Some(dir) = dir {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

pub fn shell_command(command: &str, cwd: &Path) -> Command {
    #[cfg(windows)]
    let (program, flag) = ("cmd", "/C");
    #[cfg(not(windows))]
    let (program, flag) = ("sh", "-c");
    let mut cmd = Command::new(program);
    cmd.arg(flag).arg(command).current_dir(cwd);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_both_streams_and_lossy_decodes() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("printf 'a\\377b'; printf err >&2; exit 3");
        let out = run_command(cmd, Duration::from_secs(10)).unwrap();
        assert_eq!(out.code, 3);
        assert_eq!(out.stdout, "a\u{fffd}b");
        assert_eq!(out.stderr, "err");
        assert!(!out.ok());
    }

    #[test]
    fn kills_a_command_that_outlives_its_timeout() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 30");
        let err = run_command(cmd, Duration::from_millis(100)).unwrap_err();
        assert!(err.starts_with("timed out after"), "{err}");
    }

    #[test]
    fn waits_out_a_command_that_finishes_between_polls() {
        for delay in ["0", "0.003", "0.02", "0.05", "0.15"] {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(format!(
                "sleep {delay}; printf done; printf warn >&2; exit 7"
            ));
            let out = run_command(cmd, Duration::from_secs(20)).unwrap();
            assert_eq!(
                (out.code, out.stdout.as_str(), out.stderr.as_str()),
                (7, "done", "warn"),
                "{delay}"
            );
        }
    }

    #[test]
    fn drains_output_larger_than_a_pipe_while_waiting() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(
            "head -c 300000 /dev/zero | tr '\\0' a; head -c 200000 /dev/zero | tr '\\0' b >&2",
        );
        let out = run_command(cmd, Duration::from_secs(20)).unwrap();
        assert_eq!((out.stdout.len(), out.stderr.len()), (300_000, 200_000));
        assert!(out.stdout.bytes().all(|b| b == b'a'));
        assert!(out.stderr.bytes().all(|b| b == b'b'));
    }

    #[test]
    fn a_timeout_is_reported_whole_seconds_and_stops_the_command_promptly() {
        for millis in [0u64, 1, 30, 250] {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg("sleep 30");
            let started = Instant::now();
            let err = run_command(cmd, Duration::from_millis(millis)).unwrap_err();
            assert_eq!(err, "timed out after 0s", "{millis}");
            assert!(started.elapsed() < Duration::from_secs(5), "{millis}");
        }
    }

    #[test]
    fn reports_a_missing_binary() {
        let err = run_command(
            Command::new("definitely-not-a-binary"),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(is_not_found(&err), "{err}");
    }

    #[test]
    fn git_stdout_wraps_failures_with_the_subcommand() {
        let dir = std::env::temp_dir().join(format!("exec-git-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = git_stdout(
            Some(&dir),
            &["rev-parse", "--verify", "HEAD"],
            Duration::from_secs(10),
        )
        .unwrap_err();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(err.starts_with("git rev-parse failed:"), "{err}");
    }

    #[test]
    fn shell_command_runs_in_the_given_directory() {
        let dir = std::env::temp_dir();
        let out = run_command(shell_command("pwd", &dir), Duration::from_secs(10)).unwrap();
        assert_eq!(
            std::fs::canonicalize(out.stdout.trim()).unwrap(),
            std::fs::canonicalize(&dir).unwrap()
        );
    }
}
