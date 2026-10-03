//! Explicit filesystem roots so every operation can run against a temp tree.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Roots {
    pub home: PathBuf,
    pub claude_home: PathBuf,
    pub claude_json: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub cf_dir: PathBuf,
    pub env_claude_config_dir: Option<String>,
    pub env_auto_compact_window: Option<String>,
    pub managed_settings: Option<PathBuf>,
}

impl Roots {
    pub fn from_home(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            claude_home: home.join(".claude"),
            claude_json: home.join(".claude.json"),
            config_dir: home.join(".config/mcpm"),
            cache_dir: home.join(".cache/mcpm/context"),
            cf_dir: home.join(".local/share/cf-dev-tools"),
            env_claude_config_dir: None,
            env_auto_compact_window: None,
            managed_settings: None,
        }
    }

    pub fn context_config_path(&self) -> PathBuf {
        self.config_dir.join("context.json")
    }

    pub fn profiles_root(&self) -> PathBuf {
        self.config_dir.join("claude-profiles")
    }

    pub fn shims_path(&self) -> PathBuf {
        self.config_dir.join("context-shims.zsh")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.cache_dir.join("backups")
    }

    pub fn expand_user(&self, raw: &str) -> PathBuf {
        if raw == "~" {
            return self.home.clone();
        }
        match raw.strip_prefix("~/") {
            Some(rest) => self.home.join(rest),
            None => PathBuf::from(raw),
        }
    }

    /// The clone the skills pipeline transpiles from, so layers ride the existing sync/publish flow.
    pub fn skills_repo_path(&self) -> PathBuf {
        let sync_cfg = self.config_dir.join("skills_sync.json");
        if let Ok(text) = fs::read_to_string(sync_cfg) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(local) = value.get("local_path").and_then(|v| v.as_str()) {
                    if !local.is_empty() {
                        return self.expand_user(local);
                    }
                }
            }
        }
        self.config_dir.join("skills_repo")
    }

    pub fn rules_dir(&self) -> PathBuf {
        self.skills_repo_path().join("rules")
    }
}
