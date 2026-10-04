//! The `claude` binary as a mockable runner. Only `plugin ...` sub-commands go through it.

use crate::plus::update::exec::{is_not_found, run_command, CmdOutput};
use std::process::Command;
use std::time::Duration;

pub const NOT_FOUND: &str = "claude not found on PATH";

pub trait ClaudeRunner: Sync {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<CmdOutput, String>;
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

impl ClaudeRunner for SystemClaude {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(args);
        run_command(cmd, timeout).map_err(|e| {
            if is_not_found(&e) {
                NOT_FOUND.to_string()
            } else {
                e
            }
        })
    }
}
