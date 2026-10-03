//! The provider seam as plain data: what each provider's runtime looks like and which
//! activation artifacts it generates. Probing a live engine is not done here.

use super::model::*;
use super::shims::{shell_env_snippet, shim_snippet, ShimOptions};
use super::store::Paths;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeSpec {
    pub kind: RuntimeKind,
    pub port: u16,
    pub env: OrderedMap<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: String,
    pub mode: u32,
}

pub fn runtime_spec(provider: ProviderName, config: &CompressionConfig) -> RuntimeSpec {
    match provider {
        ProviderName::Headroom => {
            let preset = config.preset_for(None);
            RuntimeSpec {
                kind: RuntimeKind::Proxy,
                port: preset.port,
                env: env_for_preset(config, &preset),
            }
        }
        // rtk's hook and parsec's plugin settings are owned by those tools, not a launch env.
        other => RuntimeSpec {
            kind: other.default_runtime(),
            port: 0,
            env: OrderedMap::new(),
        },
    }
}

/// The MCP server a provider wants registered. Only headroom has one; parsec ships its own.
pub fn mcp_server_config(provider: ProviderName) -> Option<Value> {
    (provider == ProviderName::Headroom).then(|| {
        json!({
            "name": "headroom",
            "command": "headroom",
            "args": ["mcp", "serve"],
            "proxy_mode": "direct",
        })
    })
}

pub fn activation_artifacts(
    provider: ProviderName,
    config: &CompressionConfig,
    paths: &Paths,
) -> Vec<GeneratedFile> {
    if provider != ProviderName::Headroom {
        return Vec::new();
    }
    let env = env_for_preset(config, &config.preset_for(None));
    vec![
        GeneratedFile {
            path: paths.env_snippet(),
            content: shell_env_snippet(&env),
            mode: 0o600,
        },
        GeneratedFile {
            path: paths.shims(),
            content: shim_snippet(ShimOptions::default()),
            mode: 0o644,
        },
    ]
}
