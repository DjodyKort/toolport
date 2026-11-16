//! Where the context files live, from an explicit home so tests run in temp dirs.

use super::fsx;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Roots {
    pub home: PathBuf,
    pub config_dir: PathBuf,
}

impl Roots {
    pub fn from_home(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            config_dir: home.join(".config/mcpm"),
        }
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

    pub fn context_config_path(&self) -> PathBuf {
        self.config_dir.join("context.json")
    }

    pub fn skills_repo_path(&self) -> PathBuf {
        let sync_cfg = self.config_dir.join("skills_sync.json");
        let local = fsx::read_text(&sync_cfg, fsx::TEXT_CAP)
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.get("local_path")?.as_str().map(String::from))
            .filter(|path| !path.is_empty());
        match local {
            Some(local) => self.expand_user(&local),
            None => self.config_dir.join("skills_repo"),
        }
    }

    pub fn rules_dir(&self) -> PathBuf {
        self.skills_repo_path().join("rules")
    }
}
