//! The `claude` binary as a mockable runner: `plugin list|update` and `plugin marketplace remove`
//! go through it, and nothing else.

use crate::skills::gitexec::{is_not_found, run_command, CmdOutput};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub const NOT_FOUND: &str = "claude not found on PATH";
pub const ENV_CLAUDE_BIN: &str = "TOOLPORT_CLAUDE_BIN";

pub trait ClaudeRunner: Sync {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<CmdOutput, String>;

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
