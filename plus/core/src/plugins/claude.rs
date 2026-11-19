//! The `claude` binary as a mockable runner: `plugin list|details|configure|update|validate` and
//! `plugin marketplace update|remove` go through it, and nothing else.

use crate::skills::gitexec::{is_not_found, run_command_input, CmdOutput};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub const NOT_FOUND: &str = "claude not found on PATH";
pub const ENV_CLAUDE_BIN: &str = "TOOLPORT_CLAUDE_BIN";

pub trait ClaudeRunner: Sync {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<CmdOutput, String>;

    /// [`ClaudeRunner::run`] with `input` on stdin, for values that must not appear in an
    /// argument list (`plugin configure --values-stdin`).
    fn run_input(
        &self,
        args: &[&str],
        input: Option<&str>,
        timeout: Duration,
    ) -> Result<CmdOutput, String> {
        match input {
            None => self.run(args, timeout),
            Some(_) => Err("this runner cannot take input".to_string()),
        }
    }

    /// Whether the binary can be found, without starting it.
    fn present(&self) -> bool {
        true
    }
}

pub struct SystemClaude {
    bin: String,
}

impl SystemClaude {
    pub fn from_env() -> Self {
        let bin = std::env::var(ENV_CLAUDE_BIN)
            .ok()
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "claude".to_string());
        Self { bin }
    }

    pub fn with_bin(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }
}

impl ClaudeRunner for SystemClaude {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
        self.run_input(args, None, timeout)
    }

    fn run_input(
        &self,
        args: &[&str],
        input: Option<&str>,
        timeout: Duration,
    ) -> Result<CmdOutput, String> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(args);
        run_command_input(cmd, input, timeout).map_err(|e| {
            if is_not_found(&e) {
                NOT_FOUND.to_string()
            } else {
                e
            }
        })
    }

    fn present(&self) -> bool {
        let bin = Path::new(&self.bin);
        if bin.components().count() > 1 {
            return bin.is_file();
        }
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path)
                .any(|dir| dir.join(bin).is_file() || dir.join(bin).with_extension("exe").is_file())
        })
    }
}
