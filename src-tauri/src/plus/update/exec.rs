use crate::plus::exec::{git_command, shell_command};
pub use crate::plus::exec::{is_not_found, run_command, CmdOutput};
use std::path::Path;
use std::time::Duration;

pub trait GitRunner {
    fn git(&self, repo: &Path, args: &[&str], timeout: Duration) -> Result<CmdOutput, String>;
}

pub trait ShellRunner {
    fn run(&self, command: &str, cwd: &Path, timeout: Duration) -> Result<CmdOutput, String>;
}

pub struct SystemGit;

impl GitRunner for SystemGit {
    fn git(&self, repo: &Path, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
        run_command(git_command(Some(repo), args), timeout).map_err(|e| {
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
        run_command(shell_command(command, cwd), timeout)
    }
}
