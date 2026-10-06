#![allow(dead_code)]

//! A synthetic world for tests that run the real `toolportctl`: data directory, home, skills
//! repository and a stub `claude`. Nothing in it is a real credential.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const SECRET_KEY: &str = "abababababababababababababababababababababababababababababababab";
pub const FAKE_SECRET: &str = "FAKE-SECRET-VALUE-do-not-print-7f3a";

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct CtlWorld {
    pub base: PathBuf,
    pub data: PathBuf,
    pub home: PathBuf,
    pub repo: PathBuf,
    pub claude: PathBuf,
    pub mock: String,
}

impl CtlWorld {
    pub fn new(tag: &str, mock: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "ctl-world-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let world = Self {
            data: base.join("data"),
            home: base.join("home"),
            repo: base.join("skills-repo"),
            claude: base.join("fake-claude"),
            base,
            mock: mock.to_string(),
        };
        for dir in [&world.data, &world.home, &world.home.join(".claude")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        write_json(
            &world.data.join("registry.json"),
            &json!({
                "version": 1,
                "servers": [
                    {"id": "srv-alpha", "name": "alpha", "transport": "stdio",
                     "command": mock, "args": [],
                     "mcpmSource": {"type": "unknown", "reason": "synthetic fixture"},
                     "env": [{"key": "API_KEY", "value": FAKE_SECRET, "secret": true}]},
                    {"id": "srv-beta", "name": "beta", "transport": "http",
                     "url": "https://example.invalid/mcp"}
                ],
                "profiles": [{"id": "default", "name": "Default",
                              "enabledServerIds": ["srv-alpha"]}],
                "activeProfileId": "default"
            }),
        );
        write_json(
            &world.home.join(".cursor/mcp.json"),
            &json!({"mcpServers": {"direct-one": {"command": "echo", "args": ["x"]}}}),
        );
        for (rel, text) in [
            (
                "skills/demo/SKILL.md",
                "---\nname: demo\ndescription: A synthetic demo skill\n---\nBody text\n",
            ),
            (
                "agents/helper/AGENT.md",
                "---\nname: helper\ndescription: A synthetic helper agent\nmodel: inherit\n---\nAgent prompt\n",
            ),
            (
                "styles/plain/STYLE.md",
                "---\nname: plain\ndescription: A synthetic plain style\nkeep-coding-instructions: true\n---\nStyle text\n",
            ),
        ] {
            let path = world.repo.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        super::exec::write_executable(
            &world.claude,
            "#!/bin/sh\necho '[{\"name\":\"demo-plugin\",\"marketplace\":\"fake-market\",\"version\":\"1.0.0\"}]'\n",
        );
        world
    }

    pub fn path(&self, p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    /// The environment a child `toolportctl` runs in: nothing inherited from the real machine.
    pub fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("PATH", "/usr/bin:/bin".to_string()),
            ("HOME", self.path(&self.home)),
            ("XDG_CONFIG_HOME", self.path(&self.home.join(".config"))),
            ("XDG_DATA_HOME", self.path(&self.home.join(".local/share"))),
            ("XDG_CACHE_HOME", self.path(&self.home.join(".cache"))),
            ("CLAUDE_CONFIG_DIR", self.path(&self.home.join(".claude"))),
            ("TOOLPORT_DATA_DIR", self.path(&self.data)),
            ("TOOLPORT_SECRET_KEY", SECRET_KEY.to_string()),
            ("TOOLPORT_CLAUDE_BIN", self.path(&self.claude)),
            (
                "TOOLPORT_CLAUDE_MANAGED_SETTINGS",
                self.path(&self.base.join("managed-settings.json")),
            ),
            ("TOOLPORT_SOURCES_TIME_SCALE", "20".to_string()),
            // After every hardcoded fixture timestamp (latest: run-fixture-waiting's
            // 2026-10-05T08:00:01Z in ctl_fixtures.rs), so feed items whose `since` is the
            // fixed fixture clock still sort before items whose `since` is this fake "now"
            // (attention-ls.default golden depends on that order).
            ("TOOLPORT_ATTENTION_FAKE_NOW", "1798761600".to_string()),
            ("TOOLPORT_GITLEAKS_BIN", "/nonexistent/gitleaks".to_string()),
        ]
    }

    /// Every file of the scratch tree, for "this step changed nothing" checks. Lock files and
    /// git's own bookkeeping are left out: a plan that fetches or checks out in a clone moves
    /// FETCH_HEAD and ORIG_HEAD, and what a user can see lies outside `.git`. So are the call log
    /// and version file of the `claude` stub, which record what a step asked and are not state.
    pub fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn collect(dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().is_none_or(|name| name != ".git")
                        && !path.ends_with("plus/cache")
                    {
                        collect(&path, files);
                    }
                } else if path.extension().is_some_and(|ext| ext == "lock")
                    || path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with("claude-stub."))
                {
                    continue;
                } else if let Ok(bytes) = std::fs::read(&path) {
                    files.insert(path, bytes);
                }
            }
        }
        let mut files = BTreeMap::new();
        collect(&self.base, &mut files);
        files
    }

    pub fn registry(&self) -> Value {
        read_json(&self.data.join("registry.json"))
    }
}

impl Drop for CtlWorld {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

pub fn write_json(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

pub fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
