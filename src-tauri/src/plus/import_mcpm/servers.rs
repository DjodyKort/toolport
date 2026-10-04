use super::ids::{short_id, tool_name_fits};
use crate::registry::{
    arg_looks_secret, ArgBinding, ArgPart, EnvVar, LaunchConfig, LaunchInput, ServerEntry,
};
use crate::secrets::HTTP_AUTH_KEY;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};

pub const IMPORT_SOURCE: &str = "imported:mcpm";

const PLAIN_ENV: &[&str] = &[
    "GOOGLE_MCP_PROFILE",
    "ODOO_URL",
    "ODOO_DB",
    "ODOO_USER",
    "MOODLE_URL",
    "MIRO_TOOLS_PROFILE",
    "IMAGE_DIR",
    "ANNAS_DOWNLOAD_PATH",
    "FRAMELINK_TELEMETRY",
    "GOOGLE_CLOUD_PROJECT",
    "STITCH_USE_SYSTEM_GCLOUD",
];

const RELOCATE_DIRS: &[&str] = &[".config/mcpm/bin/", ".claude/mcp-servers/"];

#[derive(Debug, Clone, Default)]
pub struct McpmInput {
    pub servers: Map<String, Value>,
    pub sources: Map<String, Value>,
    pub home: String,
    pub short_ids: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SecretWrite {
    pub server_id: String,
    pub key: String,
    pub value: String,
}

impl SecretWrite {
    pub fn vault_key(&self) -> String {
        format!("{}::{}", self.server_id, self.key)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub server: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MappedServer {
    pub entry: ServerEntry,
    #[serde(skip)]
    pub secrets: Vec<SecretWrite>,
    pub oauth_via_gateway: bool,
    #[serde(skip)]
    pub mcpm_name: String,
    #[serde(skip)]
    pub tags: Vec<String>,
    #[serde(skip)]
    pub enabled: bool,
}

fn warn(server: &str, kind: &str, detail: impl Into<String>) -> Warning {
    Warning {
        server: server.to_string(),
        kind: kind.to_string(),
        detail: detail.into(),
    }
}

pub fn map_servers(input: &McpmInput) -> (Vec<MappedServer>, Vec<Warning>) {
    let mut taken = HashSet::new();
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for (key, raw) in &input.servers {
        let Some(obj) = raw.as_object() else {
            warnings.push(warn(key, "invalid", "server entry is not an object"));
            continue;
        };
        let name = obj.get("name").and_then(Value::as_str).unwrap_or(key);
        let id = short_id(name, &input.short_ids, &taken);
        taken.insert(id.clone());
        out.push(map_one(&id, name, obj, input, &mut warnings));
    }
    (out, warnings)
}

fn str_vec(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn map_one(
    id: &str,
    name: &str,
    obj: &Map<String, Value>,
    input: &McpmInput,
    warnings: &mut Vec<Warning>,
) -> MappedServer {
    let tags = str_vec(obj.get("profile_tags"));
    let enabled = obj.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let mut secrets = Vec::new();
    let mut unknown = Map::new();
    let mut entry = ServerEntry {
        id: id.to_string(),
        name: name.to_string(),
        transport: "stdio".into(),
        command: None,
        args: Vec::new(),
        launch: None,
        env: Vec::new(),
        url: None,
        cwd: None,
        source: Some(IMPORT_SOURCE.into()),
        disabled_tools: Vec::new(),
        client_credentials: None,
        request_timeout_ms: None,
        max_request_timeout_ms: None,
        initialize_timeout_ms: None,
        unknown_fields: Map::new(),
    };
    let mut oauth = false;

    if let Some(url) = obj.get("url").and_then(Value::as_str) {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        entry.transport = if path.trim_end_matches('/').ends_with("/sse") {
            "sse"
        } else {
            "http"
        }
        .into();
        entry.url = Some(url.to_string());
        let mut bearer = false;
        if let Some(headers) = obj.get("headers").and_then(Value::as_object) {
            for (hk, hv) in headers {
                let value = hv.as_str().unwrap_or_default();
                if hk.eq_ignore_ascii_case("authorization") {
                    let token = strip_bearer(value);
                    if !token.is_empty() {
                        bearer = true;
                        secrets.push(SecretWrite {
                            server_id: id.to_string(),
                            key: HTTP_AUTH_KEY.into(),
                            value: token.to_string(),
                        });
                    }
                } else {
                    warnings.push(warn(id, "unsupported-header", hk.clone()));
                }
            }
        }
        oauth = !bearer;
    } else {
        let home = input.home.trim_end_matches('/');
        let command = obj.get("command").and_then(Value::as_str).map(String::from);
        let mut args = str_vec(obj.get("args"));
        for path in command.iter().chain(args.iter()) {
            if RELOCATE_DIRS
                .iter()
                .any(|d| path.starts_with(&format!("{home}/{d}")))
            {
                warnings.push(warn(id, "relocate-path", path.clone()));
            }
        }
        let mut launch = LaunchConfig::default();
        for (index, arg) in args.iter_mut().enumerate() {
            if arg_looks_secret(arg) {
                let key = format!("ARG_{index}");
                secrets.push(SecretWrite {
                    server_id: id.to_string(),
                    key: key.clone(),
                    value: std::mem::replace(arg, "<launch-input>".into()),
                });
                launch.inputs.push(LaunchInput {
                    key: key.clone(),
                    label: format!("Argument {index}"),
                    secret: true,
                    required: true,
                    value: None,
                });
                launch.bindings.push(ArgBinding {
                    index,
                    parts: vec![ArgPart::Input { key }],
                });
            }
        }
        entry.command = command;
        entry.args = args;
        if !launch.inputs.is_empty() {
            entry.launch = Some(launch);
        }
        if let Some(env) = obj.get("env").and_then(Value::as_object) {
            for (k, v) in env {
                let value = v.as_str().unwrap_or_default();
                if PLAIN_ENV.contains(&k.as_str()) {
                    entry.env.push(EnvVar {
                        key: k.clone(),
                        value: Some(value.to_string()),
                        secret: false,
                    });
                } else {
                    entry.env.push(EnvVar {
                        key: k.clone(),
                        value: None,
                        secret: true,
                    });
                    if !value.is_empty() {
                        secrets.push(SecretWrite {
                            server_id: id.to_string(),
                            key: k.clone(),
                            value: value.to_string(),
                        });
                    }
                }
            }
        }
    }

    if oauth {
        unknown.insert("mcpmOauth".into(), Value::Bool(true));
    }
    if !tags.is_empty() {
        unknown.insert("mcpmTags".into(), serde_json::json!(tags));
    }
    if let Some(src) = input.sources.get(name) {
        unknown.insert("mcpmSource".into(), src.clone());
    }
    if obj
        .get("requires_session_pinning")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        warnings.push(warn(id, "session-pinning", "no Toolport equivalent"));
    }
    if !enabled {
        warnings.push(warn(id, "disabled", "excluded from every profile"));
    }
    if !tool_name_fits(id, "x") {
        warnings.push(warn(id, "id-too-long", id.to_string()));
    }
    entry.unknown_fields = unknown;
    MappedServer {
        entry,
        secrets,
        oauth_via_gateway: oauth,
        mcpm_name: name.to_string(),
        tags,
        enabled,
    }
}

fn strip_bearer(value: &str) -> &str {
    let v = value.trim();
    match v.get(..7) {
        Some(p) if p.eq_ignore_ascii_case("bearer ") => v[7..].trim(),
        _ => v,
    }
}
