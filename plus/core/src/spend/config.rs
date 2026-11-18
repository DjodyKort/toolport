use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::*;

pub const CONFIG_FILE: &str = "config.json";

/// What the Spend settings hold. Unknown keys are kept, so a newer build's file survives an
/// older one's write.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendConfig {
    pub enabled: bool,
    pub port: u16,
    pub retention_days: u32,
    pub max_mb: u32,
    pub show_cost: bool,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Default for SpendConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            retention_days: DEFAULT_RETENTION_DAYS,
            max_mb: DEFAULT_MAX_MB,
            show_cost: true,
            other: Map::new(),
        }
    }
}

impl SpendConfig {
    pub fn max_bytes(&self) -> u64 {
        u64::from(self.max_mb) * 1024 * 1024
    }

    pub fn validate(&self) -> Result<(), String> {
        let (d_lo, d_hi) = RETENTION_DAYS_RANGE;
        let (m_lo, m_hi) = MAX_MB_RANGE;
        if !(d_lo..=d_hi).contains(&self.retention_days) {
            return Err(format!("retention must be {d_lo} to {d_hi} days"));
        }
        if !(m_lo..=m_hi).contains(&self.max_mb) {
            return Err(format!("the size cap must be {m_lo} to {m_hi} MB"));
        }
        if self.port < 1024 {
            return Err("the port must be 1024 or higher".to_string());
        }
        Ok(())
    }
}

pub fn load(dir: &Path) -> SpendConfig {
    std::fs::read_to_string(dir.join(CONFIG_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, config: &SpendConfig) -> Result<(), String> {
    config.validate()?;
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())? + "\n";
    crate::fsx::atomic_write(&dir.join(CONFIG_FILE), &text, crate::fsx::Mode::Private)
        .map_err(|e| format!("cannot write {}: {e}", dir.join(CONFIG_FILE).display()))
}
