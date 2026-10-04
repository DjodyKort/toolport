//! What an installed plugin folder says about itself: manifest, components, hooks files, MCP and
//! LSP servers and the option schema. Files only; every path from a manifest stays below the
//! plugin folder.

use crate::plus::sources::budget::Budget;
use crate::plus::sources::{fsx, layout};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

const CAP: usize = 4 * 1024 * 1024;
const MASK: &str = "[redacted]";

pub fn read_doc(path: &Path) -> Option<Value> {
    serde_json::from_str(&fsx::read_text(path, CAP)?).ok()
}

/// `.claude-plugin/plugin.json`, else `plugin.json`, else `null`.
pub fn load(install: &Path) -> Value {
    [".claude-plugin/plugin.json", "plugin.json"]
        .iter()
        .find_map(|rel| read_doc(&install.join(rel)))
        .filter(Value::is_object)
        .unwrap_or(Value::Null)
}

pub fn description(manifest: &Value) -> String {
    manifest
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn version(manifest: &Value) -> Option<String> {
    manifest
        .get("version")
        .and_then(Value::as_str)
        .map(String::from)
}

fn inside(install: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel.strip_prefix("./").unwrap_or(rel));
    rel.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| install.join(rel))
}

#[derive(Clone, Debug, Default)]
pub struct Components {
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub commands: Vec<String>,
}

/// The default layout: `skills/<name>/SKILL.md`, `agents/**.md`, `commands/**.md`.
pub fn components(install: &Path) -> Components {
    let mut out = Components::default();
    for found in layout::scan_layout(install, &Budget::unlimited()) {
        match found.kind {
            "skill" => out.skills.push(found.fallback),
            "agent" => out.agents.push(found.fallback),
            "command" => out.commands.push(found.fallback),
            _ => {}
        }
    }
    out
}

/// A document the manifest points at, or an inline value, with the file it came from.
#[derive(Clone, Debug)]
pub struct Piece {
    pub file: PathBuf,
    pub value: Value,
}

fn pieces(install: &Path, manifest_file: &Path, spec: Option<&Value>) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut push = |one: &Value| match one {
        Value::String(rel) => {
            if let Some(file) = inside(install, rel) {
                if let Some(value) = read_doc(&file) {
                    out.push(Piece { file, value });
                }
            }
        }
        Value::Object(_) => out.push(Piece {
            file: manifest_file.to_path_buf(),
            value: one.clone(),
        }),
        _ => {}
    };
    match spec {
        Some(Value::Array(list)) => list.iter().for_each(&mut push),
        Some(other) => push(other),
        None => {}
    }
    out
}

fn manifest_file(install: &Path) -> PathBuf {
    [".claude-plugin/plugin.json", "plugin.json"]
        .iter()
        .map(|rel| install.join(rel))
        .find(|p| fsx::is_file(p))
        .unwrap_or_else(|| install.join(".claude-plugin/plugin.json"))
}

/// The hooks documents of a plugin: `hooks/hooks.json` and whatever the manifest's `hooks` adds.
pub fn hook_docs(install: &Path, manifest: &Value) -> Vec<Piece> {
    let mut docs: Vec<Piece> = Vec::new();
    let default = install.join("hooks/hooks.json");
    if let Some(value) = read_doc(&default) {
        docs.push(Piece {
            file: default,
            value,
        });
    }
    for piece in pieces(install, &manifest_file(install), manifest.get("hooks")) {
        let same = docs
            .iter()
            .any(|d| fsx::canonical(&d.file) == fsx::canonical(&piece.file));
        if !same {
            docs.push(piece);
        }
    }
    docs
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpServer {
    pub name: String,
    /// Arguments that look like secrets are masked; `raw_command` keeps them for rule matching only.
    pub command: Option<Vec<String>>,
    pub url: Option<String>,
    pub raw_command: Option<Vec<String>>,
    pub raw_url: Option<String>,
}

/// `{mcpServers: {...}}` or a bare map of servers.
fn server_map(value: &Value) -> Option<&Map<String, Value>> {
    match value.get("mcpServers") {
        Some(Value::Object(map)) => Some(map),
        Some(_) => None,
        None => value.as_object(),
    }
}

pub fn mask_args(args: Vec<String>) -> Vec<String> {
    let secret = |flag: &str| {
        let lower = flag.trim_start_matches('-').to_ascii_lowercase();
        [
            "token",
            "secret",
            "password",
            "passwd",
            "api-key",
            "api_key",
            "apikey",
            "auth",
            "private-key",
            "credential",
        ]
            .iter()
            .any(|s| lower.contains(s))
    };
    let mut hide_next = false;
    args.into_iter()
        .map(|arg| {
            if std::mem::take(&mut hide_next) {
                return MASK.to_string();
            }
            if let Some((flag, _)) = arg.split_once('=').filter(|(f, _)| f.starts_with('-')) {
                if secret(flag) {
                    return format!("{flag}={MASK}");
                }
            } else if arg.starts_with('-') && secret(&arg) {
                hide_next = true;
            }
            arg
        })
        .collect()
}

/// A URL without credentials and query string.
pub fn clean_url(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let rest = match rest.split_once('/') {
        Some((auth, path)) => format!("{}/{path}", auth.rsplit('@').next().unwrap_or(auth)),
        None => rest.rsplit('@').next().unwrap_or(rest).to_string(),
    };
    if scheme.is_empty() {
        rest
    } else {
        format!("{scheme}://{rest}")
    }
}

fn server(name: &str, spec: &Value) -> McpServer {
    let raw_command = spec.get("command").and_then(Value::as_str).map(|cmd| {
        let mut parts = vec![cmd.to_string()];
        if let Some(args) = spec.get("args").and_then(Value::as_array) {
            parts.extend(
                args.iter()
                    .map(|a| a.as_str().map(String::from).unwrap_or_else(|| a.to_string())),
            );
        }
        parts
    });
    let raw_url = spec.get("url").and_then(Value::as_str).map(String::from);
    McpServer {
        name: name.to_string(),
        command: raw_command.clone().map(|c| {
            let mut c = c.into_iter();
            let head: Vec<String> = c.next().into_iter().collect();
            head.into_iter().chain(mask_args(c.collect())).collect()
        }),
        url: raw_url.as_deref().map(clean_url),
        raw_command,
        raw_url,
    }
}

/// The MCP servers a plugin declares: `.mcp.json` and the manifest's `mcpServers`. Environment
/// blocks are never read into the result.
pub fn mcp_servers(install: &Path, manifest: &Value) -> Vec<McpServer> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let default = install.join(".mcp.json");
    let mut docs: Vec<Value> = read_doc(&default).into_iter().collect();
    docs.extend(
        pieces(install, &manifest_file(install), manifest.get("mcpServers"))
            .into_iter()
            .map(|p| p.value),
    );
    for doc in &docs {
        let Some(map) = server_map(doc) else { continue };
        for (name, spec) in map {
            if spec.is_object() && seen.insert(name.clone()) {
                out.push(server(name, spec));
            }
        }
    }
    out
}

/// The tool prefix Claude Code gives a plugin's MCP server.
pub fn tool_prefix(plugin: &str, server: &str) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .collect()
    };
    format!("mcp__plugin_{}_{}__", clean(plugin), clean(server))
}

pub fn lsp_servers(install: &Path, manifest: &Value) -> Vec<String> {
    let mut names = BTreeSet::new();
    let default = install.join(".lsp.json");
    let mut docs: Vec<Value> = read_doc(&default).into_iter().collect();
    docs.extend(
        pieces(install, &manifest_file(install), manifest.get("lspServers"))
            .into_iter()
            .map(|p| p.value),
    );
    for doc in docs {
        let map = match doc.get("lspServers") {
            Some(Value::Object(map)) => Some(map.clone()),
            Some(_) => None,
            None => doc.as_object().cloned(),
        };
        names.extend(map.into_iter().flatten().filter(|(_, v)| v.is_object()).map(|(k, _)| k));
    }
    names.into_iter().collect()
}

/// One option a plugin asks the user for. The value of a sensitive option is never held.
#[derive(Clone, Debug, PartialEq)]
pub struct OptionRow {
    pub key: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub default: Value,
    pub current: Option<String>,
    pub choices: Option<Vec<String>>,
    pub sensitive: bool,
    pub configured: bool,
}

impl OptionRow {
    pub fn from_schema(key: &str, schema: &Value) -> Self {
        let text = |field: &str| {
            schema
                .get(field)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let kind = match text("type") {
            t if t.is_empty() => "string".to_string(),
            t => t,
        };
        Self {
            key: key.to_string(),
            title: match text("title") {
                t if t.is_empty() => key.to_string(),
                t => t,
            },
            description: text("description"),
            choices: (kind == "boolean").then(|| vec!["true".to_string(), "false".to_string()]),
            default: schema.get("default").cloned().unwrap_or(Value::Null),
            sensitive: schema.get("sensitive").and_then(Value::as_bool) == Some(true),
            kind,
            current: None,
            configured: false,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "key": self.key,
            "title": self.title,
            "description": self.description,
            "type": self.kind,
            "default": if self.sensitive { Value::Null } else { self.default.clone() },
            "current": if self.sensitive { None } else { self.current.clone() },
            "choices": self.choices,
            "sensitive": self.sensitive,
            "configured": self.configured,
        })
    }
}

/// The options the manifest declares (`userConfig`), in key order.
pub fn user_config(manifest: &Value) -> Vec<OptionRow> {
    manifest
        .get("userConfig")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter(|(_, v)| v.is_object())
                .map(|(k, v)| OptionRow::from_schema(k, v))
                .collect()
        })
        .unwrap_or_default()
}

pub fn value_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_paths_cannot_leave_the_plugin_folder() {
        let base = Path::new("/p");
        assert_eq!(inside(base, "./hooks/a.json"), Some(PathBuf::from("/p/hooks/a.json")));
        assert_eq!(inside(base, "../x.json"), None);
        assert_eq!(inside(base, "/etc/passwd"), None);
        assert_eq!(inside(base, "a/../../x"), None);
    }

    #[test]
    fn secret_looking_arguments_are_masked() {
        let args = ["--port", "1", "--api-key", "abc", "--token=abc", "--name", "x"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            mask_args(args),
            ["--port", "1", "--api-key", MASK, &format!("--token={MASK}"), "--name", "x"]
        );
    }

    #[test]
    fn urls_lose_credentials_and_query() {
        assert_eq!(
            clean_url("https://user:pw@example.test/mcp?token=abc#x"),
            "https://example.test/mcp"
        );
        assert_eq!(clean_url("https://example.test"), "https://example.test");
    }

    #[test]
    fn tool_prefix_replaces_odd_characters() {
        assert_eq!(
            tool_prefix("ecc", "chrome-devtools"),
            "mcp__plugin_ecc_chrome-devtools__"
        );
        assert_eq!(tool_prefix("a.b", "c d"), "mcp__plugin_a_b_c_d__");
    }

    #[test]
    fn a_sensitive_option_never_serialises_a_value() {
        let mut row = OptionRow::from_schema(
            "api_token",
            &json!({"type":"string","title":"Token","default":"d","sensitive":true}),
        );
        row.current = Some("s3cret".into());
        let out = row.to_json();
        assert_eq!(out["current"], Value::Null);
        assert_eq!(out["default"], Value::Null);
        assert_eq!(out["sensitive"], true);
        assert!(!out.to_string().contains("s3cret"));
    }

    #[test]
    fn boolean_options_offer_true_and_false() {
        let row = OptionRow::from_schema("on", &json!({"type":"boolean","default":true}));
        assert_eq!(row.choices, Some(vec!["true".into(), "false".into()]));
        assert_eq!(row.title, "on");
    }
}
