//! Adopting the policy mcpm kept in `~/.config/mcpm/compression.json`. The file is only read:
//! a data directory that has no policy yet starts from it (the pre-pin shape is migrated by
//! `store::parse` exactly as mcpm did on load), and one that has a policy never takes it.

use super::model::CompressionConfig;
use super::store::{self, Paths, CONFIG_FILE};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// mcpm's config directory: `MCPM_CONFIG_DIR`, else `~/.config/mcpm`.
pub fn default_root() -> Option<PathBuf> {
    match std::env::var_os("MCPM_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => dirs::home_dir().map(|home| home.join(".config").join("mcpm")),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Adoption {
    pub from: PathBuf,
    pub notes: Vec<String>,
}

impl Adoption {
    pub fn to_value(&self) -> Value {
        json!({"from": self.from.to_string_lossy(), "notes": self.notes})
    }
}

#[derive(Debug)]
pub struct Prepared {
    pub config: CompressionConfig,
    pub adoption: Option<Adoption>,
    pub warnings: Vec<String>,
}

/// The policy a mutating command starts from. The caller's own save persists an adoption.
pub fn prepare(paths: &Paths, root: Option<&Path>) -> Result<Prepared, String> {
    let loaded = store::read(paths)?;
    let mut out = Prepared {
        config: loaded.config,
        adoption: None,
        warnings: Vec::new(),
    };
    let legacy = match root {
        Some(root) if !loaded.existed => root.join(CONFIG_FILE),
        _ => return Ok(out),
    };
    let text = match std::fs::read_to_string(&legacy) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => {
            out.warnings
                .push(format!("ignored legacy {}: {e}", legacy.display()));
            return Ok(out);
        }
    };
    match store::parse(&text) {
        Ok((config, notes)) => {
            out.config = config;
            out.adoption = Some(Adoption {
                from: legacy,
                notes,
            });
        }
        Err(why) => out
            .warnings
            .push(format!("ignored legacy {}: {why}", legacy.display())),
    }
    Ok(out)
}
