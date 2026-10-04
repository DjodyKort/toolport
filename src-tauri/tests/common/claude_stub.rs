#![allow(dead_code)]

//! The `claude` stand-in of the `context measure` tests: a wrapper that runs
//! `fixtures/loads/claude-stub.sh` with a call log and a version file, so a test can count the
//! requests that were made and change the version Claude Code reports.

use super::exec::write_executable;
use std::path::{Path, PathBuf};

pub fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/loads/claude-stub.sh")
}

pub struct ClaudeStub {
    pub bin: PathBuf,
    log: PathBuf,
    version: PathBuf,
}

impl ClaudeStub {
    /// Writes the wrapper at `bin`; the log and the version file go next to each other in `dir`.
    pub fn install(bin: &Path, dir: &Path) -> Self {
        let stub = Self {
            bin: bin.to_path_buf(),
            log: dir.join("claude-stub.log"),
            version: dir.join("claude-stub.version"),
        };
        let _ = std::fs::remove_file(&stub.log);
        write_executable(
            bin,
            &format!(
                "#!/bin/sh\nSTUB_CLAUDE_LOG='{}'\nSTUB_CLAUDE_VERSION_FILE='{}'\nexport STUB_CLAUDE_LOG STUB_CLAUDE_VERSION_FILE\nexec sh '{}' \"$@\"\n",
                stub.log.display(),
                stub.version.display(),
                script().display()
            ),
        );
        stub
    }

    pub fn set_version(&self, version: &str) {
        std::fs::write(&self.version, version).unwrap();
    }

    pub fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The calls that would have cost model tokens.
    pub fn requests(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|c| c.starts_with("request"))
            .collect()
    }
}
