//! The `claude` binary as a mockable runner. Only `plugin ...` sub-commands go through it.

use crate::plus::exec::run_command_input;
use crate::plus::update::exec::{is_not_found, run_command, CmdOutput};
use std::process::Command;
use std::time::Duration;

pub const NOT_FOUND: &str = "claude not found on PATH";

pub trait ClaudeRunner: Sync {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<CmdOutput, String>;

    /// Like `run`, with `input` on the child's stdin: the way values reach `claude` without
    /// showing up in an argument list.
    fn run_with_input(&self, _args: &[&str], _input: &str, _timeout: Duration) -> Result<CmdOutput, String> {
        Err("this runner cannot feed stdin".to_string())
    }
}

pub struct SystemClaude {
    bin: String,
}

impl SystemClaude {
    pub fn from_env() -> Self {
        let bin = std::env::var("TOOLPORT_CLAUDE_BIN")
            .ok()
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "claude".to_string());
        Self { bin }
    }

    #[cfg(test)]
    pub fn with_bin(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }
}

impl SystemClaude {
    fn start(&self, args: &[&str], input: Option<&str>, timeout: Duration) -> Result<CmdOutput, String> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(args);
        let out = match input {
            Some(_) => run_command_input(cmd, input, timeout),
            None => run_command(cmd, timeout),
        };
        out.map_err(|e| if is_not_found(&e) { NOT_FOUND.to_string() } else { e })
    }
}

impl ClaudeRunner for SystemClaude {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
        self.start(args, None, timeout)
    }

    fn run_with_input(&self, args: &[&str], input: &str, timeout: Duration) -> Result<CmdOutput, String> {
        self.start(args, Some(input), timeout)
    }
}
