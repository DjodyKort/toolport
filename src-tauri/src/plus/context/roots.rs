//! Explicit filesystem roots so every operation can run against a temp tree.

use super::config::ContextConfig;
use crate::plus::jsonfs::read_json;
use std::path::{Path, PathBuf};

pub const ENV_CORP_TOOLS_DIR: &str = "TOOLPORT_CORP_TOOLS_DIR";
pub const ENV_CLIENTS_ROOT: &str = "TOOLPORT_CLIENTS_ROOT";
const DEFAULT_CORP_TOOLS_REL: &str = ".local/share/corp-dev-tools";
pub const CONTEXT_SHIMS_FILE: &str = "context-shims.zsh";

#[derive(Clone, Debug)]
pub struct Roots {
    pub home: PathBuf,
    pub claude_home: PathBuf,
    pub claude_json: PathBuf,
    pub config_dir: PathBuf,
    /// Where Toolport writes its generated shell files. `from_home` keeps mcpm's directory so the
    /// replayed mcpm goldens stay byte-identical; `roots_from_args` points it at the data dir.
    pub shims_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub cf_dir: PathBuf,
    pub env_corp_tools_dir: Option<String>,
    pub env_clients_root: Option<String>,
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
            shims_dir: home.join(".config/mcpm"),
            cache_dir: home.join(".cache/mcpm/context"),
            cf_dir: home.join(DEFAULT_CORP_TOOLS_REL),
            env_corp_tools_dir: None,
            env_clients_root: None,
            env_claude_config_dir: None,
            env_auto_compact_window: None,
            managed_settings: None,
        }
    }

    pub fn read_env(&mut self) {
        self.env_corp_tools_dir = std::env::var(ENV_CORP_TOOLS_DIR).ok();
        self.env_clients_root = std::env::var(ENV_CLIENTS_ROOT).ok();
    }

    pub fn resolved(&self, config: &ContextConfig) -> Self {
        let mut out = self.clone();
        out.cf_dir = self.resolve_corp_tools_dir(config);
        out
    }

    pub fn resolve_corp_tools_dir(&self, config: &ContextConfig) -> PathBuf {
        if let Some(raw) = self.env_corp_tools_dir.clone().filter(|v| !v.is_empty()) {
            return self.expand_user(&raw);
        }
        match config.corp_tools_dir.as_deref().filter(|v| !v.is_empty()) {
            Some(raw) => self.expand_user(raw),
            None => self.cf_dir.clone(),
        }
    }

    pub fn resolve_clients_root(&self, config: &ContextConfig) -> PathBuf {
        match self.env_clients_root.clone().filter(|v| !v.is_empty()) {
            Some(raw) => self.expand_user(&raw),
            None => self.expand_user(&config.clients_root),
        }
    }

    pub fn default_cf_dir(&self) -> PathBuf {
        self.home.join(DEFAULT_CORP_TOOLS_REL)
    }

    pub fn context_config_path(&self) -> PathBuf {
        self.config_dir.join("context.json")
    }

    pub fn profiles_root(&self) -> PathBuf {
        self.config_dir.join("claude-profiles")
    }

    pub fn shims_path(&self) -> PathBuf {
        self.shims_dir.join(CONTEXT_SHIMS_FILE)
    }

    pub fn legacy_shims_path(&self) -> PathBuf {
        self.config_dir.join(CONTEXT_SHIMS_FILE)
    }

    pub fn zshrc_path(&self) -> PathBuf {
        self.home.join(".zshrc")
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
        match read_json::<serde_json::Value>(&sync_cfg) {
            Some(value) => match value.get("local_path").and_then(|v| v.as_str()) {
                Some(local) if !local.is_empty() => self.expand_user(local),
                _ => self.config_dir.join("skills_repo"),
            },
            None => self.config_dir.join("skills_repo"),
        }
    }

    pub fn rules_dir(&self) -> PathBuf {
        self.skills_repo_path().join("rules")
    }
}
