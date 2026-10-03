use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn default_files() -> Vec<String> {
    vec!["CLAUDE.md".to_string()]
}

fn default_branch() -> String {
    "main".to_string()
}

fn default_backend() -> String {
    "git".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncProjectConfig {
    pub local_path: String,
    #[serde(default = "default_files")]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default)]
    pub machine_id: String,
    #[serde(default)]
    pub repo_url: Option<String>,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default)]
    pub cloud_url: Option<String>,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub projects: BTreeMap<String, SyncProjectConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncStateEntry {
    pub local_hash_at_sync: String,
    pub remote_hash_at_sync: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncState {
    #[serde(default)]
    pub last_sync_at: String,
    #[serde(default)]
    pub last_direction: String,
    #[serde(default)]
    pub entries: BTreeMap<String, SyncStateEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerOrigin {
    pub server_name: String,
    pub origin_type: String,
    #[serde(default)]
    pub git_url: Option<String>,
    #[serde(default = "default_origin_branch")]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub github_repo: Option<String>,
    #[serde(default)]
    pub asset_pattern: Option<String>,
    #[serde(default)]
    pub install_path: Option<String>,
    #[serde(default)]
    pub setup_command: Option<String>,
}

fn default_origin_branch() -> Option<String> {
    Some("main".to_string())
}

impl ServerOrigin {
    pub fn plain(name: &str, origin_type: &str) -> Self {
        Self {
            server_name: name.to_string(),
            origin_type: origin_type.to_string(),
            git_url: None,
            git_branch: default_origin_branch(),
            github_repo: None,
            asset_pattern: None,
            install_path: None,
            setup_command: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerOrigins {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub servers: BTreeMap<String, ServerOrigin>,
}

fn one() -> u32 {
    1
}

impl Default for ServerOrigins {
    fn default() -> Self {
        Self {
            version: 1,
            servers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    pub entry_key: String,
    pub local_hash: String,
    pub remote_hash: String,
    pub remote_file_saved_as: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Changes {
    pub new: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
    pub conflicts: Vec<String>,
    pub unchanged: Vec<String>,
}

impl Changes {
    pub fn is_clean(&self) -> bool {
        self.new.is_empty()
            && self.modified.is_empty()
            && self.removed.is_empty()
            && self.conflicts.is_empty()
    }
}
