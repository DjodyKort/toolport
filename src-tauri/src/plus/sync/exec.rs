use crate::plus::exec::{git_stdout, run_command, shell_command};
use std::path::Path;
use std::time::Duration;

pub trait Exec {
    fn git(&self, dir: Option<&Path>, args: &[&str]) -> Result<String, String>;

    fn shell(&self, dir: &Path, command: &str) -> Result<(), String>;
}

pub struct SystemExec;

const GIT_TIMEOUT: Duration = Duration::from_secs(60);
const SHELL_TIMEOUT: Duration = Duration::from_secs(120);

impl Exec for SystemExec {
    fn git(&self, dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
        git_stdout(dir, args, GIT_TIMEOUT)
    }

    fn shell(&self, dir: &Path, command: &str) -> Result<(), String> {
        let out = run_command(shell_command(command, dir), SHELL_TIMEOUT)?;
        if out.ok() {
            Ok(())
        } else {
            Err(out.stderr.trim().chars().take(200).collect())
        }
    }
}
