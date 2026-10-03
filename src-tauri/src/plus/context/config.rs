//! Declarative context config (`context.json`), compatible with mcpm-context's pydantic schema.

use crate::plus::jsonfs::read_json;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const CF_LEGACY_SERVER_NAMES: [&str; 4] = [
    "context7",
    "playwright",
    "examplecorp-odoo",
    "examplecorp-typst",
];

fn yes() -> bool {
    true
}

fn inherit() -> Value {
    Value::String("inherit".into())
}

fn import() -> String {
    "import".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileSpec {
    #[serde(default = "yes")]
    pub org: bool,
    #[serde(default = "import")]
    pub org_mode: String,
    #[serde(default = "inherit")]
    pub rules: Value,
    #[serde(default = "yes")]
    pub commands: bool,
    #[serde(default = "yes")]
    pub skills: bool,
    #[serde(default = "yes")]
    pub agents: bool,
    #[serde(default = "inherit")]
    pub servers: Value,
    #[serde(default)]
    pub settings_overrides: Map<String, Value>,
    #[serde(default = "yes")]
    pub copy_auth: bool,
    #[serde(default = "yes")]
    pub link_rules: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_window: Option<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_windows: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<super::compact::CheckpointSpec>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub autocompact_flag: bool,
}

impl Default for ProfileSpec {
    fn default() -> Self {
        serde_json::from_value(Value::Object(Map::new())).expect("defaults")
    }
}

fn default_legacy_names() -> Vec<String> {
    CF_LEGACY_SERVER_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DedupePolicy {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_legacy_names")]
    pub legacy_names: Vec<String>,
    #[serde(default = "yes")]
    pub require_mcpm_twin: bool,
}

impl Default for DedupePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            legacy_names: default_legacy_names(),
            require_mcpm_twin: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsPolicy {
    #[serde(default)]
    pub ensure_allow: Vec<String>,
    #[serde(default)]
    pub ensure_ask: Vec<String>,
}

fn default_clients_root() -> String {
    "~/Documents/GitHub/ExampleWorkspace/clients".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextConfig {
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileSpec>,
    #[serde(default = "yes")]
    pub wrap_default_claude: bool,
    #[serde(default)]
    pub dedupe: DedupePolicy,
    #[serde(default)]
    pub settings: SettingsPolicy,
    #[serde(default = "default_clients_root")]
    pub clients_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corp_tools_dir: Option<String>,
    #[serde(default)]
    pub cf_wrapper_hash: Option<String>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            profiles: BTreeMap::new(),
            wrap_default_claude: true,
            dedupe: DedupePolicy::default(),
            settings: SettingsPolicy::default(),
            clients_root: default_clients_root(),
            corp_tools_dir: None,
            cf_wrapper_hash: None,
        }
    }
}

fn valid_profile_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    let alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    alnum(bytes[0]) && bytes.iter().all(|&b| alnum(b) || b == b'-')
}

impl ContextConfig {
    /// Profile names become zsh function names, so anything outside `[a-z0-9][a-z0-9-]*` is refused.
    pub fn validate(&self) -> Result<(), String> {
        for (name, spec) in &self.profiles {
            if !valid_profile_name(name) {
                return Err(format!(
                    "profile name {name:?} must match [a-z0-9][a-z0-9-]* (it becomes a shell function)"
                ));
            }
            super::compact::validate(name, spec)?;
        }
        Ok(())
    }

    pub fn from_value(value: Value) -> Result<Self, String> {
        let config: Self =
            serde_json::from_value(value).map_err(|e| format!("invalid context config: {e}"))?;
        config.validate()?;
        Ok(config)
    }

    pub fn to_json_text(&self) -> String {
        let text = serde_json::to_string(self).expect("config serializes");
        let tree = crate::plus::skills::json::parse(&text).expect("round trip");
        format!("{}\n", tree.dumps())
    }
}

fn parse_config(path: &Path) -> Option<ContextConfig> {
    read_json::<Value>(path)
        .and_then(|v| ContextConfig::from_value(v).ok())
}

/// A corrupt or old config falls back to defaults rather than failing, like mcpm.
pub fn load_config(path: &Path) -> ContextConfig {
    parse_config(path).unwrap_or_default()
}

/// Copies a config that [`load_config`] would silently replace with defaults, so persisting
/// those defaults does not destroy what the user wrote. Returns the copy's path.
pub fn preserve_unreadable(path: &Path) -> Option<std::path::PathBuf> {
    if !path.is_file() || parse_config(path).is_some() {
        return None;
    }
    crate::registry::quarantine_existing(path)
}

pub fn save_config(path: &Path, config: &ContextConfig) -> Result<(), String> {
    crate::registry::atomic_write(path, &config.to_json_text())
        .map_err(|e| format!("{}: {e}", path.display()))
}
